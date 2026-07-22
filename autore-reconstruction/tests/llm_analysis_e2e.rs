//! End-to-end integration test: LLM analysis function slice with mock fallback.
//!
//! Boots the `openai-compatible` provider against a mock local LLM HTTP
//! endpoint, dispatches `llm.analysis.function`, collects the raw + parsed
//! observations, and runs the `LlmImporter` to assert canonical mutations
//! flow through `ApplicationCommand`.
//!
//! Opt-in real LLM: set `AUTORE_TEST_REAL_LLM_ENDPOINT` to skip the mock
//! server and forward to a real endpoint.
//!
//! Run with:
//! ```text
//! PROTOC=/tmp/opencode/protoc/bin/protoc cargo test -p autore-reconstruction \
//!     --test llm_analysis_e2e -- --ignored --nocapture
//! ```

#[path = "../src/tests_support.rs"]
#[allow(dead_code)]
mod tests_support;

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

use autore_app::ApplicationCommand;
use autore_provider_protocol::v1::ExecutionRequest;
use autore_provider_protocol::v1::execution_event;
use autore_provider_runtime::runtime::{ProviderConfigBundle, ProviderManifest, ProviderRuntime};
use autore_reconstruction::analysis::{
    InvestigationBundle, LlmImportResult, LlmImporter, request_payload_for,
};
use autore_reconstruction::{CallSiteSummary, DependencyEdgeKind};
use autore_schema::ids::{ArtifactId, EntityId, ProjectId, WorkItemId};
use serde_json::Value;
use tests_support::RecordingAutoReClient;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

// ---------------------------------------------------------------------------
// Mock HTTP server
// ---------------------------------------------------------------------------

async fn start_mock_llm_server(response_body: String) -> (SocketAddr, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind mock");
    let addr = listener.local_addr().expect("local addr");
    println!("mock-listening-on={addr}");

    let handle = tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                break;
            };
            let body = response_body.clone();
            tokio::spawn(async move {
                let _ = serve_one_request(stream, &body).await;
            });
        }
    });
    (addr, handle)
}

async fn serve_one_request(
    mut stream: tokio::net::TcpStream,
    response_body: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut buf = vec![0u8; 65536];
    let mut total = 0usize;
    loop {
        let n = stream.read(&mut buf[total..]).await?;
        if n == 0 {
            return Ok(());
        }
        total += n;
        if find_crlf2(&buf[..total]).is_some() {
            break;
        }
    }
    let hdr_end = find_crlf2(&buf[..total]).unwrap();
    let hdrs = std::str::from_utf8(&buf[..hdr_end])?;
    let cl = parse_content_length(hdrs).unwrap_or(0);
    let body_off = hdr_end + 4;
    while total < body_off + cl {
        let n = stream.read(&mut buf[total..]).await?;
        if n == 0 {
            break;
        }
        total += n;
    }
    let resp = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{}",
        response_body.len(),
        response_body
    );
    stream.write_all(resp.as_bytes()).await?;
    let _ = stream.shutdown().await;
    Ok(())
}

fn find_crlf2(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n")
}

fn parse_content_length(hdrs: &str) -> Option<usize> {
    hdrs.lines().find_map(|line| {
        line.strip_prefix("Content-Length:")
            .or_else(|| line.strip_prefix("content-length:"))
            .and_then(|v| v.trim().parse().ok())
    })
}

// ---------------------------------------------------------------------------
// Provider binary
// ---------------------------------------------------------------------------

fn ensure_provider_binary() -> PathBuf {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root")
        .to_path_buf();
    let bin = if cfg!(windows) {
        "openai-compatible-provider.exe"
    } else {
        "openai-compatible-provider"
    };
    let path = root.join("target").join("debug").join(bin);
    if path.exists() {
        return path;
    }
    let protoc =
        std::env::var("PROTOC").unwrap_or_else(|_| "/tmp/opencode/protoc/bin/protoc".into());
    eprintln!("[llm_e2e] building provider binary...");
    let status = std::process::Command::new("cargo")
        .args(["build", "-p", "openai-compatible-provider"])
        .env("PROTOC", &protoc)
        .current_dir(&root)
        .status()
        .expect("cargo build");
    assert!(
        status.success(),
        "cargo build -p openai-compatible-provider failed"
    );
    assert!(path.exists(), "binary missing at {}", path.display());
    path
}

// ---------------------------------------------------------------------------
// Bundle + mock response
// ---------------------------------------------------------------------------

fn build_test_bundle() -> InvestigationBundle {
    InvestigationBundle {
        subject_identity: WorkItemId::new(),
        subject_entity_id: Some(EntityId::new()),
        static_structural_snapshot: None,
        decompilation_artifact: None,
        disassembly_artifact: None,
        cfg_summary: None,
        callers_and_callees: vec![CallSiteSummary {
            work_item_id: WorkItemId::new(),
            brief: "caller of subject".into(),
            edge_kind: DependencyEdgeKind::DirectCall,
        }],
        relevant_types: vec![EntityId::new()],
        relevant_globals: vec![],
        strings_and_constants: vec![],
        dynamic_observations: vec![],
        accepted_hypotheses: vec![],
        unresolved_conflicts: vec![],
        prior_generated_candidate: None,
        compiler_diagnostics: vec![],
        verification_failures: vec![],
        requested_output_schema: Value::Null,
    }
}

fn build_mock_openai_response(entity_id: &str) -> String {
    let inner = serde_json::json!({
        "proposed_name": "add",
        "behavior_claims": ["adds two integers and returns the result"],
        "side_effects": [],
        "signature": "int add(int a, int b)",
        "evidence_references": [entity_id],
        "confidence": 0.92,
        "recommended_follow_up_work": ["verify overflow behavior at 0x1160"]
    });
    serde_json::json!({
        "choices": [{ "message": { "role": "assistant", "content": inner.to_string() } }]
    })
    .to_string()
}

// ---------------------------------------------------------------------------
// Main test
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore]
async fn llm_analysis_function_e2e_mock() {
    // Step 0: real-LLM opt-in
    let real_endpoint = std::env::var("AUTORE_TEST_REAL_LLM_ENDPOINT").ok();

    // Step 1: provider binary
    let provider_bin = ensure_provider_binary();
    eprintln!("[llm_e2e] provider binary: {}", provider_bin.display());

    // Step 2: bundle + mock or real endpoint
    let bundle = build_test_bundle();
    let entity_id_str = bundle.subject_entity_id.unwrap().to_string();

    let (_mock_handle, endpoint_url) = if let Some(ref ep) = real_endpoint {
        eprintln!("[llm_e2e] real LLM endpoint: {ep}");
        (None, ep.clone())
    } else {
        let mock_resp = build_mock_openai_response(&entity_id_str);
        let (addr, h) = start_mock_llm_server(mock_resp).await;
        (Some(h), format!("http://{addr}/v1/chat/completions"))
    };

    // Step 3: bootstrap provider via gRPC runtime
    let manifest = ProviderManifest {
        executable_path: provider_bin,
        package_id: "openai-compatible".into(),
        package_version: "0.1.0".into(),
        content_hash: None,
    };
    let mut extra = HashMap::new();
    extra.insert("AUTORE_LLM_ENDPOINT".into(), endpoint_url);
    extra.insert("AUTORE_LLM_API_KEY_REF".into(), "test-key-no-secret".into());
    extra.insert("AUTORE_LLM_MODEL".into(), "test-model".into());
    extra.insert("AUTORE_LLM_TEMPERATURE".into(), "0.0".into());
    extra.insert("AUTORE_LLM_MAX_TOKENS".into(), "1024".into());
    let config = ProviderConfigBundle { extra_env: extra };

    let mut handle = ProviderRuntime::spawn(manifest, config, Duration::from_secs(30))
        .await
        .expect("provider spawn");
    eprintln!(
        "[llm_e2e] provider up: instance_id={}, caps={}",
        handle.instance_id,
        handle.capabilities.len()
    );

    // Step 4: dispatch llm.analysis.function
    let payload = request_payload_for(&bundle);
    let req = ExecutionRequest {
        request_id: "req-llm-e2e".into(),
        operation_id: "op-llm-e2e".into(),
        capability_id: "llm.analysis.function".into(),
        capability_version: "1.0.0".into(),
        payload,
        deadline: None,
    };
    let mut stream = handle
        .client
        .execute(req)
        .await
        .expect("execute RPC")
        .into_inner();

    // Step 5: collect events
    let mut events = Vec::new();
    while let Some(ev) = tokio_stream::StreamExt::next(&mut stream).await {
        events.push(ev.expect("event"));
    }
    eprintln!("[llm_e2e] collected {} events", events.len());

    // Step 6: extract raw response + parsed result
    let mut raw_text: Option<String> = None;
    let mut parsed_value: Option<Value> = None;
    let mut succeeded = false;
    for ev in &events {
        match &ev.event {
            Some(execution_event::Event::ObservationProduced(o)) => {
                match o.observation_kind.as_str() {
                    "llm.raw-response" => {
                        raw_text = Some(String::from_utf8_lossy(&o.payload).into_owned())
                    }
                    "llm.parsed-result" => {
                        parsed_value = Some(serde_json::from_slice(&o.payload).expect("json"))
                    }
                    _ => {}
                }
            }
            Some(execution_event::Event::Completed(c)) => {
                succeeded =
                    c.status == autore_provider_protocol::v1::completed::Status::Succeeded as i32;
            }
            _ => {}
        }
    }
    assert!(succeeded, "execution must succeed; events={events:?}");
    let raw_text = raw_text.expect("llm.raw-response observation");
    let parsed_value = parsed_value.expect("llm.parsed-result observation");

    // Step 7: run LlmImporter (attempt_count=0)
    let client = RecordingAutoReClient::new();
    let project_id = ProjectId::new();
    let importer = LlmImporter::new(
        &bundle,
        "llm.analysis.function",
        ArtifactId::new(),
        ArtifactId::new(),
        0,
        &client,
        project_id,
        raw_text,
        parsed_value,
    );
    let result = importer.import().expect("import");

    // Step 8: assertions
    let LlmImportResult::Success {
        hypotheses,
        follow_up_work,
    } = &result
    else {
        panic!("expected Success, got {result:?}");
    };

    let evidence_n = client.count(|c| matches!(c, ApplicationCommand::AddEvidence(_)));
    assert!(evidence_n >= 1, ">=1 AddEvidence for llm.raw-response");

    let hypothesis_n = client.count(|c| matches!(c, ApplicationCommand::AddHypothesis(_)));
    assert!(hypothesis_n >= 1, ">=1 AddHypothesis");

    assert!(!hypotheses.is_empty(), ">=1 hypothesis");
    assert!(!follow_up_work.is_empty(), ">=1 follow-up work item");

    // canonical mutation audit
    for cmd in client.commands() {
        let ok = matches!(
            cmd,
            ApplicationCommand::AddEvidence(_)
                | ApplicationCommand::AddHypothesis(_)
                | ApplicationCommand::CreateWorkItems(_)
                | ApplicationCommand::FailWorkItem(_)
                | ApplicationCommand::BlockWorkWithReason(_)
        );
        assert!(ok, "all commands canonical, got: {cmd:?}");
    }

    // Step 9: summary
    println!(
        "raw=persisted; parsed=persisted; hypotheses={hypothesis_n}; follow-up-work-items={}",
        follow_up_work.len()
    );

    // Cleanup
    handle.shutdown().await.expect("shutdown");
    eprintln!("[llm_e2e] PASSED");
}
