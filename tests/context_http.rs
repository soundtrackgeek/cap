use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

use cap::context_http::ContextHttpClient;
use capsule_core::context::{CancellationToken, HttpClient, HttpRequest, NeverCancel};

fn client() -> ContextHttpClient {
    ContextHttpClient::new(Arc::new(NeverCancel)).unwrap()
}

fn listen() -> (TcpListener, HttpRequest) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let request = HttpRequest::new(
        format!(
            "http://{}/search?q=Test&key=private-value",
            listener.local_addr().unwrap()
        ),
        vec![("User-Agent", "cap-context-test")],
    );
    (listener, request)
}

fn accept(listener: &TcpListener) -> TcpStream {
    let started = Instant::now();
    let mut stream = loop {
        match listener.accept() {
            Ok((stream, _)) => break stream,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                assert!(
                    started.elapsed() < Duration::from_secs(6),
                    "request never arrived"
                );
                thread::sleep(Duration::from_millis(5));
            }
            Err(error) => panic!("{error}"),
        }
    };
    // Windows accepts inherit the listener's nonblocking mode. Read the test
    // request with the bounded blocking timeout below, even if headers arrive later.
    stream.set_nonblocking(false).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let mut headers = Vec::new();
    while !headers.ends_with(b"\r\n\r\n") {
        let mut byte = [0];
        stream.read_exact(&mut byte).unwrap();
        headers.push(byte[0]);
    }
    let headers = String::from_utf8(headers).unwrap().to_lowercase();
    assert!(headers.starts_with("get /search?q=test&key=private-value http/1.1"));
    assert!(headers.contains("user-agent: cap-context-test"));
    stream
}

fn respond(mut stream: TcpStream, status: u16, headers: &str) {
    write!(
        stream,
        "HTTP/1.1 {status} Test\r\n{headers}Content-Length: 2\r\nConnection: close\r\n\r\n[]"
    )
    .unwrap();
}

#[test]
fn transient_status_recovers_once_with_provider_request_spacing() {
    let (listener, request) = listen();
    let server = thread::spawn(move || {
        respond(accept(&listener), 503, "");
        respond(accept(&listener), 200, "");
    });
    let started = Instant::now();
    let result = client().get(request, Duration::from_secs(3)).unwrap();
    assert_eq!(result.status, 200);
    assert_eq!(result.body, b"[]");
    assert!(started.elapsed() >= Duration::from_secs(1));
    server.join().unwrap();
}

#[test]
fn connection_closed_before_headers_is_retried() {
    let (listener, request) = listen();
    let server = thread::spawn(move || {
        drop(accept(&listener));
        respond(accept(&listener), 200, "");
    });
    assert_eq!(
        client()
            .get(request, Duration::from_secs(3))
            .unwrap()
            .status,
        200
    );
    server.join().unwrap();
}

#[test]
fn stalled_first_connection_recovers_within_the_original_budget() {
    let (listener, request) = listen();
    let server = thread::spawn(move || {
        let _stalled = accept(&listener);
        respond(accept(&listener), 200, "");
    });
    let started = Instant::now();
    let result = client().get(request, Duration::from_secs(4)).unwrap();
    assert_eq!(result.status, 200);
    assert!(started.elapsed() < Duration::from_secs(4));
    server.join().unwrap();
}

#[test]
fn repeated_timeouts_keep_the_total_budget_and_report_the_cause_without_credentials() {
    let (listener, request) = listen();
    let (done, finished) = std::sync::mpsc::channel();
    let server = thread::spawn(move || {
        let _first = accept(&listener);
        let _second = accept(&listener);
        finished.recv_timeout(Duration::from_secs(4)).unwrap();
    });
    let started = Instant::now();
    let error = client().get(request, Duration::from_secs(2)).unwrap_err();
    let elapsed = started.elapsed();
    done.send(()).unwrap();
    server.join().unwrap();
    assert!(elapsed < Duration::from_secs(3), "{elapsed:?}");
    assert!(error.contains("timed out"), "{error}");
    assert!(error.contains("2 attempt(s)"), "{error}");
    assert!(
        !error.contains("private-value") && !error.contains("127.0.0.1"),
        "{error}"
    );
}

#[test]
fn successful_permanent_and_retry_after_responses_are_not_retried() {
    for (status, headers) in [
        (200, ""),
        (400, ""),
        (403, ""),
        (429, "Retry-After: 120\r\n"),
        (503, "Retry-After: invalid\r\n"),
    ] {
        let (listener, request) = listen();
        let server = thread::spawn(move || respond(accept(&listener), status, headers));
        let result = client().get(request, Duration::from_secs(3)).unwrap();
        assert_eq!(result.status, status);
        server.join().unwrap();
    }
}

#[test]
fn incomplete_rate_limit_body_does_not_discard_retry_after() {
    let (listener, request) = listen();
    let server = thread::spawn(move || {
        let mut stream = accept(&listener);
        stream.write_all(b"HTTP/1.1 429 Test\r\nRetry-After: 120\r\nContent-Length: 100\r\nConnection: close\r\n\r\n").unwrap();
    });
    let result = client().get(request, Duration::from_secs(3)).unwrap();
    assert_eq!(result.status, 429);
    server.join().unwrap();
}

#[test]
fn cancellation_prevents_the_retry_during_backoff() {
    let (listener, request) = listen();
    let cancellation = Arc::new(CancellationToken::default());
    let server_cancellation = cancellation.clone();
    let server = thread::spawn(move || {
        respond(accept(&listener), 503, "");
        thread::sleep(Duration::from_millis(100));
        server_cancellation.cancel();
    });
    let started = Instant::now();
    let error = ContextHttpClient::new(cancellation)
        .unwrap()
        .get(request, Duration::from_secs(3))
        .unwrap_err();
    assert_eq!(error, "context capture cancelled");
    assert!(started.elapsed() < Duration::from_secs(1));
    server.join().unwrap();
}
