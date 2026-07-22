//! Integration tests for the OpenAI-compatible provider.
//!
//! All tests use a deterministic mock responder so no real LLM endpoint is
//! required. The tests assert on the stream of `ExecutionEvent` values
//! produced by the provider for a given capability invocation.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use autore_provider_protocol::v1::provider_server::Provider;
use autore_provider_protocol::v1::{ExecutionRequest, NegotiateRequest, execution_event};
use openai_compatible_provider::llm::{HttpResponse, LlmError, MockResponder, OpenAiClient};
use openai_compatible_provider::prompts::PromptRegistry;
use openai_compatible_provider::provider::CAPABILITIES;
use openai_compatible_provider::provider::test_support::provider_for_tests;
use tempfile::TempDir;
use tonic::Request;

fn write_minimal_templates(dir: &std::path::Path) {
    for id in CAPABILITIES {
        let slug = id.replace('.', "_");
        let body = format!("CAPABILITY={id}\n{{{{bundle}}}}\n");
        std::fs::write(dir.join(format!("{slug}.handlebars")), body).unwrap();
    }
    std::fs::write(
        dir.join("schema_repair.handlebars"),
        "REPAIR\n{{{bundle}}}\n{{{invalid}}}\n{{{errors}}}\n",
    )
    .unwrap();
}

fn investigation_bundle() -> Vec<u8> {
    serde_json::json!({
        "subject_entity_id": "019abcde-0000-7000-8000-000000000001",
        "evidence_references": [],
        "relevant_entity_ids": [],
        "context_bytes_b64": "",
        "config_hints": {}
    })
    .to_string()
    .into_bytes()
}

fn make_request(capability_id: &str) -> ExecutionRequest {
    ExecutionRequest {
        request_id: "req-1".into(),
        operation_id: "op-1".into(),
        capability_id: capability_id.into(),
        capability_version: "1.0.0".into(),
        payload: investigation_bundle(),
        deadline: None,
    }
}

fn valid_openai_body() -> String {
    serde_json::json!({
        "choices": [{
            "message": {
                "role": "assistant",
                "content": serde_json::json!({
                    "proposed_name": "sub_401000",
                    "behavior_claims": ["reads from input buffer"],
                    "side_effects": [],
                    "signature": "void sub_401000(void)",
                    "evidence_references": [],
                    "confidence": 0.7,
                    "recommended_follow_up_work": []
                }).to_string()
            }
        }]
    })
    .to_string()
}

#[test]
fn negotiate_advertises_thirteen_capabilities() {
    let tmp = TempDir::new().unwrap();
    write_minimal_templates(tmp.path());
    let prompts = PromptRegistry::load(tmp.path());
    let client = OpenAiClient::with_mock(MockResponder(|| async move {
        Err::<HttpResponse, LlmError>(LlmError::Timeout)
    }));
    let staging_tmp = TempDir::new().unwrap();
    let provider = provider_for_tests(
        "instance-1",
        prompts,
        client,
        staging_tmp.path().to_path_buf(),
    );

    let rt = tokio::runtime::Runtime::new().unwrap();
    let resp = rt
        .block_on(provider.negotiate(Request::new(NegotiateRequest {
            min_supported: 1,
            max_supported: 1,
            coordinator_id: "coord".into(),
        })))
        .expect("negotiate ok")
        .into_inner();

    assert_eq!(resp.accepted_version, 1);
    assert_eq!(
        resp.capabilities.len(),
        13,
        "must advertise 13 capabilities"
    );
    let mut cap_ids: Vec<&str> = resp
        .capabilities
        .iter()
        .map(|c| c.capability_id.as_str())
        .collect();
    cap_ids.sort();
    let mut expected: Vec<&str> = CAPABILITIES.to_vec();
    expected.sort();
    assert_eq!(cap_ids, expected);

    for cap in &resp.capabilities {
        assert!(
            !cap.request_schema.is_empty(),
            "request_schema must not be empty for {}",
            cap.capability_id
        );
        assert!(
            !cap.response_schema.is_empty(),
            "response_schema must not be empty for {}",
            cap.capability_id
        );
    }
}

#[test]
fn provider_renders_capability_specific_prompt() {
    let tmp = TempDir::new().unwrap();
    write_minimal_templates(tmp.path());
    let prompts = PromptRegistry::load(tmp.path());

    let mut rendered = Vec::new();
    for id in CAPABILITIES {
        let out = prompts.render(id, r#"{"subject_entity_id":"e1"}"#).unwrap();
        assert!(
            out.contains(id),
            "rendered prompt for {id} must reference the capability id; got {out}"
        );
        rendered.push(out);
    }

    for i in 0..rendered.len() {
        for j in (i + 1)..rendered.len() {
            assert_ne!(
                rendered[i], rendered[j],
                "prompts for {} and {} must differ",
                CAPABILITIES[i], CAPABILITIES[j]
            );
        }
    }
}

#[test]
fn provider_submits_with_response_format_json_schema() {
    let captured: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
    let cap_clone = captured.clone();

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut buf = [0u8; 65536];
        let n = stream.read(&mut buf).unwrap_or(0);
        let mut body = cap_clone.lock().unwrap();
        *body = buf[..n].to_vec();
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            valid_openai_body().len(),
            valid_openai_body()
        );
        let _ = stream.write_all(response.as_bytes());
    });

    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let tmp = TempDir::new().unwrap();
        write_minimal_templates(tmp.path());
        let prompts = PromptRegistry::load(tmp.path());
        let client = OpenAiClient::new(
            format!("http://{addr}/v1/chat/completions"),
            "plain-text-test-key-not-from-env".into(),
            "mock-model".into(),
            0.0,
            256,
        );
        let staging_tmp = TempDir::new().unwrap();
        let provider = provider_for_tests(
            "instance-1",
            prompts,
            client,
            staging_tmp.path().to_path_buf(),
        );
        let req = make_request("llm.analysis.function");
        let resp = provider
            .execute(Request::new(req))
            .await
            .unwrap()
            .into_inner();
        let events: Vec<_> = tokio_stream::StreamExt::collect(resp).await;
        let last = events.last().expect("non-empty stream").as_ref().unwrap();
        match &last.event {
            Some(execution_event::Event::Completed(c)) => {
                assert_eq!(
                    c.status,
                    autore_provider_protocol::v1::completed::Status::Succeeded as i32
                );
            }
            other => panic!("expected Completed, got {other:?}"),
        }
    });

    let body = captured.lock().unwrap();
    let body_str = String::from_utf8_lossy(&body);
    assert!(
        body_str.contains("\"response_format\""),
        "request body must include response_format: {body_str}"
    );
    assert!(
        body_str.contains("\"json_schema\""),
        "request body must include json_schema in response_format: {body_str}"
    );
    assert!(
        body_str.contains("llm_analysis_function"),
        "request body must carry the capability schema name: {body_str}"
    );
}

#[test]
fn provider_retries_once_on_malformed_output() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = call_count.clone();
    let mock = MockResponder(move || {
        let cc = cc.clone();
        async move {
            let _ = cc.fetch_add(1, Ordering::SeqCst);
            Ok::<HttpResponse, LlmError>(HttpResponse {
                status: 200,
                body: "{".to_string(),
            })
        }
    });

    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let tmp = TempDir::new().unwrap();
        write_minimal_templates(tmp.path());
        let prompts = PromptRegistry::load(tmp.path());
        let mut client = OpenAiClient::with_mock(mock);
        client.set_api_key_ref("test-key-for-retry".into());
        let staging_tmp = TempDir::new().unwrap();
        let provider = provider_for_tests(
            "instance-1",
            prompts,
            client,
            staging_tmp.path().to_path_buf(),
        );
        let req = make_request("llm.analysis.function");
        let resp = provider
            .execute(Request::new(req))
            .await
            .unwrap()
            .into_inner();
        let events: Vec<_> = tokio_stream::StreamExt::collect(resp).await;
        let last = events.last().expect("non-empty stream").as_ref().unwrap();
        match &last.event {
            Some(execution_event::Event::Completed(c)) => {
                assert_eq!(
                    c.status,
                    autore_provider_protocol::v1::completed::Status::Failed as i32,
                    "malformed output must yield Completed{{Failed}}"
                );
            }
            other => panic!("expected Completed, got {other:?}"),
        }
    });

    let calls = call_count.load(Ordering::SeqCst);
    assert_eq!(
        calls, 2,
        "expected exactly 1 retry (2 total calls), got {calls}"
    );
}

#[test]
fn provider_never_persists_plaintext_key() {
    const PLAINTEXT_KEY: &str = "sk-autore-test-DO-NOT-PERSIST-0123456789abcdef";

    let mock = MockResponder(|| async move {
        Ok::<HttpResponse, LlmError>(HttpResponse {
            status: 200,
            body: valid_openai_body(),
        })
    });

    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let tmp = TempDir::new().unwrap();
        write_minimal_templates(tmp.path());
        let prompts = PromptRegistry::load(tmp.path());
        let mut client = OpenAiClient::with_mock(mock);
        client.set_api_key_ref(PLAINTEXT_KEY.to_string());

        let staging_tmp = TempDir::new().unwrap();
        let provider = provider_for_tests(
            "instance-1",
            prompts,
            client,
            staging_tmp.path().to_path_buf(),
        );
        let req = make_request("llm.analysis.function");
        let resp = provider
            .execute(Request::new(req))
            .await
            .unwrap()
            .into_inner();
        let events: Vec<_> = tokio_stream::StreamExt::collect(resp).await;

        for (i, ev_result) in events.iter().enumerate() {
            let ev = ev_result.as_ref().unwrap();
            match &ev.event {
                Some(execution_event::Event::ObservationProduced(o)) => {
                    let payload_str = String::from_utf8_lossy(&o.payload);
                    assert!(
                        !payload_str.contains(PLAINTEXT_KEY),
                        "event[{i}] observation payload must not contain plaintext key"
                    );
                }
                Some(execution_event::Event::Completed(c)) => {
                    assert!(!c.summary.contains(PLAINTEXT_KEY));
                }
                Some(execution_event::Event::Diagnostic(d)) => {
                    assert!(!d.message.contains(PLAINTEXT_KEY));
                }
                _ => {}
            }
        }
    });
}
