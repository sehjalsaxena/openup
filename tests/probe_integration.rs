use openup::probe::{ProbeResult, build_fast_client, extract_gateway_base, post_login, probe_url};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};

fn read_full_http_request(stream: &mut TcpStream) -> String {
    let _ = stream.set_nodelay(true);
    let mut data = Vec::new();
    let mut buf = [0u8; 1024];
    let mut content_length: Option<usize> = None;
    let mut header_len: Option<usize> = None;

    loop {
        let n = stream.read(&mut buf).unwrap_or(0);
        if n == 0 {
            break;
        }
        data.extend_from_slice(&buf[..n]);

        if header_len.is_none()
            && let Some(pos) = data.windows(4).position(|w| w == b"\r\n\r\n")
        {
            let h_end = pos + 4;
            header_len = Some(h_end);
            let header_str = String::from_utf8_lossy(&data[..h_end]);
            for line in header_str.lines() {
                if let Some(idx) = line.find(':') {
                    let key = line[..idx].trim().to_lowercase();
                    if key == "content-length" {
                        content_length = line[idx + 1..].trim().parse::<usize>().ok();
                    }
                }
            }
        }

        if let Some(h_end) = header_len {
            let required_total = h_end + content_length.unwrap_or(0);
            if data.len() >= required_total {
                break;
            }
        }
    }

    String::from_utf8_lossy(&data).to_string()
}

fn spawn_mock_server(raw_response: &'static str) -> (String, std::thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("Failed to bind mock listener");
    let port = listener.local_addr().unwrap().port();
    let url = format!("http://127.0.0.1:{port}/probe");
    let handle = std::thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let _ = read_full_http_request(&mut stream);
            let _ = stream.write_all(raw_response.as_bytes());
            let _ = stream.flush();
        }
    });
    (url, handle)
}

#[test]
fn test_integration_firefox_probe_online() {
    let (url, handle) = spawn_mock_server(
        "HTTP/1.1 200 OK\r\nContent-Length: 8\r\nConnection: close\r\n\r\nsuccess\n",
    );
    let client = build_fast_client(false).expect("Client build failed");
    let result = probe_url(&client, &url, None);
    handle.join().unwrap();
    assert_eq!(result, ProbeResult::OnlineUnlocked);
}

#[test]
fn test_integration_firefox_probe_302_redirect() {
    let (url, handle) = spawn_mock_server(
        "HTTP/1.1 302 Found\r\nLocation: http://172.16.68.6:8090/httpclient.html?u=http://detectportal.firefox.com/success.txt\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
    );
    let client = build_fast_client(false).expect("Client build failed");
    let result = probe_url(&client, &url, None);
    handle.join().unwrap();
    assert_eq!(
        result,
        ProbeResult::LockedPortal {
            gateway_base: "http://172.16.68.6:8090".to_string(),
            login_xml_url: "http://172.16.68.6:8090/login.xml".to_string(),
        }
    );
}

#[test]
fn test_integration_firefox_probe_html_js_redirect() {
    let body = "<script>window.location=\"https://10.1.1.9:8090/httpclient.html\"</script>";
    let resp = format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    let (url, handle) = spawn_mock_server(Box::leak(resp.into_boxed_str()));
    let client = build_fast_client(false).expect("Client build failed");
    let result = probe_url(&client, &url, None);
    handle.join().unwrap();
    assert_eq!(
        result,
        ProbeResult::LockedPortal {
            gateway_base: "https://10.1.1.9:8090".to_string(),
            login_xml_url: "https://10.1.1.9:8090/login.xml".to_string(),
        }
    );
}

#[test]
fn test_integration_extract_gateway_base() {
    let text = "<form action=\"http://172.16.68.6:8090/login.xml\" method=\"post\">";
    let base = extract_gateway_base(text);
    assert_eq!(base, Some("http://172.16.68.6:8090".to_string()));
}

#[test]
fn test_e2e_firefox_redirect_to_sophos_login_xml() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("Failed to bind e2e mock server");
    let port = listener.local_addr().unwrap().port();
    let probe_url_str = format!("http://127.0.0.1:{port}/success.txt");

    let handle = std::thread::spawn(move || {
        // Step 1: Intercept probe with 302 Found redirect
        if let Ok((mut stream1, _)) = listener.accept() {
            let _ = read_full_http_request(&mut stream1);
            let redirect_response = format!(
                "HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:{port}/httpclient.html?u=http://detectportal.firefox.com/success.txt\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            );
            let _ = stream1.write_all(redirect_response.as_bytes());
            let _ = stream1.flush();
        }

        // Step 2: Accept POST /login.xml with special chars in password and return LIVE XML
        if let Ok((mut stream2, _)) = listener.accept() {
            let req_str = read_full_http_request(&mut stream2);

            // Assert that password with & and = was properly URL-encoded
            assert!(
                req_str.contains("password=Chitkara%26Pass%3D123"),
                "Expected url-encoded password in POST body. Received request:\n{}",
                req_str
            );

            let xml_body = "<requestresponse><status><![CDATA[LIVE]]></status><message><![CDATA[You are signed in as testuser]]></message></requestresponse>";
            let login_response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/xml\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                xml_body.len(),
                xml_body
            );
            let _ = stream2.write_all(login_response.as_bytes());
            let _ = stream2.flush();
        }
    });

    let client = build_fast_client(false).expect("Client build failed");
    let probe_result = probe_url(&client, &probe_url_str, None);
    match probe_result {
        ProbeResult::LockedPortal {
            gateway_base,
            login_xml_url,
        } => {
            assert_eq!(gateway_base, format!("http://127.0.0.1:{port}"));
            assert_eq!(login_xml_url, format!("http://127.0.0.1:{port}/login.xml"));

            let login_res = post_login(&client, &login_xml_url, "testuser", "Chitkara&Pass=123");
            assert!(
                login_res.is_ok(),
                "Expected post_login to succeed, but got: {:?}",
                login_res
            );
        }
        other => panic!("Expected LockedPortal probe result, got: {:?}", other),
    }

    handle.join().unwrap();
}
