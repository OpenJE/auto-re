//! Integration test: fixture provider with 5 capabilities through ProviderRuntime::spawn.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use autore_provider_protocol::v1::{ExecutionRequest, RequestDeadline, execution_event};
use autore_provider_runtime::{ProviderConfigBundle, ProviderManifest, ProviderRuntime};
use tokio_stream::StreamExt;

/// Resolves the fixture-provider binary path from the workspace target dir.
fn fixture_provider_path() -> PathBuf {
    if let Ok(target_dir) = std::env::var("CARGO_TARGET_DIR") {
        return PathBuf::from(target_dir).join("debug/fixture-provider");
    }
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_dir.parent().unwrap();
    workspace_root.join("target/debug/fixture-provider")
}

/// Spawns the fixture provider and returns the handle.
async fn spawn_fixture() -> autore_provider_runtime::runtime::ProviderInstanceHandle {
    let binary = fixture_provider_path();
    assert!(
        binary.exists(),
        "fixture-provider binary not found at {binary:?} — run `cargo build -p fixture-provider` first"
    );

    let manifest = ProviderManifest {
        executable_path: binary,
        package_id: "fixture.echo".into(),
        package_version: "0.1.0".into(),
        content_hash: None,
    };
    let config = ProviderConfigBundle {
        extra_env: HashMap::new(),
    };

    ProviderRuntime::spawn(manifest, config, Duration::from_secs(10))
        .await
        .expect("fixture provider spawn failed")
}

/// Collects all events from an Execute stream.
async fn collect_events(
    handle: &mut autore_provider_runtime::runtime::ProviderInstanceHandle,
    req: ExecutionRequest,
) -> Vec<autore_provider_protocol::v1::ExecutionEvent> {
    let mut streaming = handle
        .client
        .execute(req)
        .await
        .expect("execute RPC failed")
        .into_inner();

    let mut events = Vec::new();
    while let Some(result) = streaming.next().await {
        events.push(result.expect("stream event error"));
    }
    events
}

/// Returns the event variant name for debugging.
fn event_variant_name(event: &autore_provider_protocol::v1::ExecutionEvent) -> &'static str {
    match &event.event {
        Some(execution_event::Event::Accepted(_)) => "Accepted",
        Some(execution_event::Event::Progress(_)) => "Progress",
        Some(execution_event::Event::Diagnostic(_)) => "Diagnostic",
        Some(execution_event::Event::ObservationProduced(_)) => "ObservationProduced",
        Some(execution_event::Event::ArtifactProduced(_)) => "ArtifactProduced",
        Some(execution_event::Event::Completed(_)) => "Completed",
        None => "None",
    }
}

/// Asserts that sequence numbers are strictly monotonic within the event list.
fn assert_monotonic_sequence(events: &[autore_provider_protocol::v1::ExecutionEvent]) {
    let sequences: Vec<u64> = events
        .iter()
        .map(|e| match &e.event {
            Some(execution_event::Event::Accepted(v)) => v.sequence,
            Some(execution_event::Event::Progress(v)) => v.sequence,
            Some(execution_event::Event::Diagnostic(v)) => v.sequence,
            Some(execution_event::Event::ObservationProduced(v)) => v.sequence,
            Some(execution_event::Event::ArtifactProduced(v)) => v.sequence,
            Some(execution_event::Event::Completed(v)) => v.sequence,
            None => 0,
        })
        .collect();

    for window in sequences.windows(2) {
        assert!(
            window[1] > window[0],
            "sequence not monotonic: {:?} → {:?}",
            window[0],
            window[1]
        );
    }
}

/// Asserts that every event carries the expected identifiers.
fn assert_event_identifiers(
    events: &[autore_provider_protocol::v1::ExecutionEvent],
    expected_instance_id: &str,
    expected_request_id: &str,
    expected_operation_id: &str,
    expected_capability_id: &str,
) {
    for (i, event) in events.iter().enumerate() {
        let variant = event_variant_name(event);
        match &event.event {
            Some(execution_event::Event::Accepted(v)) => {
                assert_eq!(
                    v.provider_instance_id, expected_instance_id,
                    "event[{i}] {variant}"
                );
                assert_eq!(v.request_id, expected_request_id, "event[{i}] {variant}");
                assert_eq!(
                    v.operation_id, expected_operation_id,
                    "event[{i}] {variant}"
                );
                assert_eq!(
                    v.capability_id, expected_capability_id,
                    "event[{i}] {variant}"
                );
            }
            Some(execution_event::Event::Progress(v)) => {
                assert_eq!(
                    v.provider_instance_id, expected_instance_id,
                    "event[{i}] {variant}"
                );
                assert_eq!(v.request_id, expected_request_id, "event[{i}] {variant}");
                assert_eq!(
                    v.operation_id, expected_operation_id,
                    "event[{i}] {variant}"
                );
                assert_eq!(
                    v.capability_id, expected_capability_id,
                    "event[{i}] {variant}"
                );
            }
            Some(execution_event::Event::Diagnostic(v)) => {
                assert_eq!(
                    v.provider_instance_id, expected_instance_id,
                    "event[{i}] {variant}"
                );
                assert_eq!(v.request_id, expected_request_id, "event[{i}] {variant}");
                assert_eq!(
                    v.operation_id, expected_operation_id,
                    "event[{i}] {variant}"
                );
                assert_eq!(
                    v.capability_id, expected_capability_id,
                    "event[{i}] {variant}"
                );
            }
            Some(execution_event::Event::ObservationProduced(v)) => {
                assert_eq!(
                    v.provider_instance_id, expected_instance_id,
                    "event[{i}] {variant}"
                );
                assert_eq!(v.request_id, expected_request_id, "event[{i}] {variant}");
                assert_eq!(
                    v.operation_id, expected_operation_id,
                    "event[{i}] {variant}"
                );
                assert_eq!(
                    v.capability_id, expected_capability_id,
                    "event[{i}] {variant}"
                );
            }
            Some(execution_event::Event::ArtifactProduced(v)) => {
                assert_eq!(
                    v.provider_instance_id, expected_instance_id,
                    "event[{i}] {variant}"
                );
                assert_eq!(v.request_id, expected_request_id, "event[{i}] {variant}");
                assert_eq!(
                    v.operation_id, expected_operation_id,
                    "event[{i}] {variant}"
                );
                assert_eq!(
                    v.capability_id, expected_capability_id,
                    "event[{i}] {variant}"
                );
            }
            Some(execution_event::Event::Completed(v)) => {
                assert_eq!(
                    v.provider_instance_id, expected_instance_id,
                    "event[{i}] {variant}"
                );
                assert_eq!(v.request_id, expected_request_id, "event[{i}] {variant}");
                assert_eq!(
                    v.operation_id, expected_operation_id,
                    "event[{i}] {variant}"
                );
                assert_eq!(
                    v.capability_id, expected_capability_id,
                    "event[{i}] {variant}"
                );
            }
            None => panic!("event[{i}] has no variant"),
        }
    }
}

fn make_request(capability_id: &str, request_id: &str) -> ExecutionRequest {
    ExecutionRequest {
        request_id: request_id.into(),
        operation_id: "op-test-001".into(),
        capability_id: capability_id.into(),
        capability_version: "1.0.0".into(),
        payload: Vec::new(),
        deadline: Some(RequestDeadline {
            absolute: None,
            relative_budget: Some(prost_types::Duration {
                seconds: 2,
                nanos: 0,
            }),
        }),
    }
}

/// Integration test: all 5 capabilities produce correct event ordering.
#[tokio::test]
async fn fixture_provider_five_capabilities() {
    let mut handle = spawn_fixture().await;
    let instance_id = handle.instance_id.to_string();

    // --- fixture.echo ---
    {
        let req = make_request("fixture.echo", "req-echo-001");
        let events = collect_events(&mut handle, req).await;
        let names: Vec<&str> = events.iter().map(event_variant_name).collect();
        assert_eq!(
            names,
            vec!["Accepted", "ObservationProduced", "Completed"],
            "echo event order"
        );
        assert_monotonic_sequence(&events);
        assert_event_identifiers(
            &events,
            &instance_id,
            "req-echo-001",
            "op-test-001",
            "fixture.echo",
        );
    }

    // --- fixture.delay ---
    {
        let req = make_request("fixture.delay", "req-delay-001");
        let events = collect_events(&mut handle, req).await;
        let names: Vec<&str> = events.iter().map(event_variant_name).collect();
        assert_eq!(
            names,
            vec!["Accepted", "Progress", "Completed"],
            "delay event order"
        );
        assert_monotonic_sequence(&events);
        assert_event_identifiers(
            &events,
            &instance_id,
            "req-delay-001",
            "op-test-001",
            "fixture.delay",
        );
    }

    // --- fixture.fail ---
    {
        let req = make_request("fixture.fail", "req-fail-001");
        let events = collect_events(&mut handle, req).await;
        let names: Vec<&str> = events.iter().map(event_variant_name).collect();
        assert_eq!(
            names,
            vec!["Accepted", "Diagnostic", "Completed"],
            "fail event order"
        );
        // Verify the Completed event has Failed status.
        if let Some(execution_event::Event::Completed(c)) = &events.last().unwrap().event {
            assert_eq!(
                c.status,
                autore_provider_protocol::v1::completed::Status::Failed as i32
            );
        } else {
            panic!("last event should be Completed");
        }
        assert_monotonic_sequence(&events);
        assert_event_identifiers(
            &events,
            &instance_id,
            "req-fail-001",
            "op-test-001",
            "fixture.fail",
        );
    }

    // --- fixture.artifact ---
    {
        let req = make_request("fixture.artifact", "req-artifact-001");
        let events = collect_events(&mut handle, req).await;
        let names: Vec<&str> = events.iter().map(event_variant_name).collect();
        assert_eq!(
            names,
            vec!["Accepted", "ArtifactProduced", "Completed"],
            "artifact event order"
        );
        // Verify the artifact descriptor is present.
        if let Some(execution_event::Event::ArtifactProduced(ap)) = &events[1].event {
            let art = ap.artifact.as_ref().expect("artifact descriptor missing");
            assert_eq!(art.size, 65536);
            assert!(!art.content_hash.is_empty());
        } else {
            panic!("second event should be ArtifactProduced");
        }
        assert_monotonic_sequence(&events);
        assert_event_identifiers(
            &events,
            &instance_id,
            "req-artifact-001",
            "op-test-001",
            "fixture.artifact",
        );
    }

    // --- fixture.large-stream ---
    {
        let req = make_request("fixture.large-stream", "req-large-001");
        let events = collect_events(&mut handle, req).await;
        // 1 Accepted + 1024 Progress + 1 Completed = 1026 events.
        assert_eq!(
            events.len(),
            1026,
            "large-stream should produce 1026 events"
        );
        assert_eq!(event_variant_name(&events[0]), "Accepted");
        assert_eq!(event_variant_name(&events[events.len() - 1]), "Completed");
        for event in &events[1..events.len() - 1] {
            assert_eq!(event_variant_name(event), "Progress");
        }
        assert_monotonic_sequence(&events);
        assert_event_identifiers(
            &events,
            &instance_id,
            "req-large-001",
            "op-test-001",
            "fixture.large-stream",
        );
    }

    // Shutdown the provider.
    handle.shutdown().await.expect("shutdown failed");
}
