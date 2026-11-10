use super::*;
use interprocess::local_socket::{prelude::*, GenericNamespaced, ListenerOptions};
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn owned_pipe_stop_response_is_confirmed_before_success() {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let cases: &[(&str, &[u8], bool)] = &[
        (
            "success",
            b"{\"id\":\"cli:session:stop\",\"result\":{\"type\":\"ok\"}}\n",
            true,
        ),
        (
            "fragmented",
            b"{\"id\":\"cli:session:stop\",\"result\":{\"type\":\"ok\"}}\n",
            true,
        ),
        (
            "error",
            b"{\"id\":\"cli:session:stop\",\"error\":{\"code\":\"denied\",\"message\":\"no\"}}\n",
            false,
        ),
        ("malformed", b"not-json\n", false),
        (
            "wrong-id",
            b"{\"id\":\"other\",\"result\":{\"type\":\"ok\"}}\n",
            false,
        ),
        ("partial", b"{\"id\":\"cli:session:stop\"", false),
        ("empty", b"", false),
        (
            "ambiguous",
            b"{\"id\":\"cli:session:stop\",\"result\":{\"type\":\"ok\"},\"error\":{}}\n",
            false,
        ),
    ];
    for (label, response, expected_ok) in cases {
        let path = format!("herdr-owned-stop-{}-{nonce}-{label}", std::process::id());
        let name = path.clone().to_ns_name::<GenericNamespaced>().unwrap();
        let listener = ListenerOptions::new()
            .name(name)
            .reclaim_name(false)
            .create_sync()
            .unwrap();
        let response = response.to_vec();
        let fragmented = *label == "fragmented";
        let worker = std::thread::spawn(move || {
            let mut stream = listener.accept().unwrap();
            let mut request = String::new();
            BufReader::new(&mut stream).read_line(&mut request).unwrap();
            assert!(request.contains("server.stop"));
            if fragmented {
                let middle = response.len() / 2;
                stream.write_all(&response[..middle]).unwrap();
                stream.flush().unwrap();
                std::thread::sleep(Duration::from_millis(40));
                stream.write_all(&response[middle..]).unwrap();
            } else {
                stream.write_all(&response).unwrap();
            }
            stream.flush().unwrap();
        });
        let result = stop_socket_with_timeout(
            path.clone().into(),
            vec![path.into()],
            Duration::from_secs(2),
            "owned fixture",
        );
        worker.join().unwrap();
        assert_eq!(result.is_ok(), *expected_ok, "{label}: {result:?}");
        if *label == "error" {
            assert!(result.unwrap_err().contains("denied"));
        } else if !expected_ok {
            assert!(result.unwrap_err().contains("outcome unknown"));
        }
    }
}

#[test]
fn response_frame_boundaries_are_checked_without_pipe_timing() {
    let mut bytes = Vec::new();
    assert!(append_stop_response_chunk(&mut bytes, &vec![b'x'; 65_536])
        .unwrap()
        .is_none());
    assert!(append_stop_response_chunk(&mut bytes, b"x")
        .unwrap_err()
        .to_string()
        .contains("too large"));
    let mut bytes = Vec::new();
    assert!(append_stop_response_chunk(&mut bytes, b"ok\nextra")
        .unwrap_err()
        .to_string()
        .contains("unexpected data"));
    let mut bytes = Vec::new();
    assert!(append_stop_response_chunk(&mut bytes, b"par")
        .unwrap()
        .is_none());
    assert_eq!(
        append_stop_response_chunk(&mut bytes, b"tial\n")
            .unwrap()
            .as_deref(),
        Some("partial")
    );
}

#[test]
fn owned_silent_response_remains_unknown() {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = format!("herdr-owned-silent-{}-{nonce}", std::process::id());
    let name = path.clone().to_ns_name::<GenericNamespaced>().unwrap();
    let listener = ListenerOptions::new()
        .name(name)
        .reclaim_name(false)
        .create_sync()
        .unwrap();
    let worker = std::thread::spawn(move || {
        let mut stream = listener.accept().unwrap();
        let mut request = String::new();
        BufReader::new(&mut stream).read_line(&mut request).unwrap();
        assert!(request.contains("server.stop"));
        std::thread::sleep(Duration::from_millis(300));
    });
    let result = stop_socket_with_timeout(
        path.clone().into(),
        vec![path.into()],
        Duration::from_millis(150),
        "owned fixture",
    );
    worker.join().unwrap();
    let error = result.unwrap_err();
    assert!(error.contains("outcome unknown") && error.contains("do not retry blindly"));
}

#[test]
fn owned_live_client_pipe_prevents_stop_confirmation() {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let api = format!("herdr-owned-live-api-{}-{nonce}", std::process::id());
    let client = format!("herdr-owned-live-client-{}-{nonce}", std::process::id());
    let client_name = client.clone().to_ns_name::<GenericNamespaced>().unwrap();
    let retained = ListenerOptions::new()
        .name(client_name)
        .reclaim_name(false)
        .create_sync()
        .unwrap();
    let api_name = api.clone().to_ns_name::<GenericNamespaced>().unwrap();
    let listener = ListenerOptions::new()
        .name(api_name)
        .reclaim_name(false)
        .create_sync()
        .unwrap();
    let worker = std::thread::spawn(move || {
        let mut stream = listener.accept().unwrap();
        let mut request = String::new();
        BufReader::new(&mut stream).read_line(&mut request).unwrap();
        assert!(request.contains("server.stop"));
        stream
            .write_all(b"{\"id\":\"cli:session:stop\",\"result\":{\"type\":\"ok\"}}\n")
            .unwrap();
        stream.flush().unwrap();
    });
    let result = stop_socket_with_timeout(
        api.clone().into(),
        vec![api.into(), client.clone().into()],
        Duration::from_millis(300),
        "owned fixture",
    );
    worker.join().unwrap();
    let error = result.unwrap_err();
    assert!(error.contains("absence could not be confirmed") && error.contains(&client));
    drop(retained);
}
