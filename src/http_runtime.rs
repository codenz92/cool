use std::process::Command;

fn run_curl(args: &[String], context: &str) -> Result<String, String> {
    let output = Command::new("curl")
        .args(args)
        .output()
        .map_err(|e| format!("{context} error: {e}"))?;

    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).to_string());
    }

    let code = output
        .status
        .code()
        .map(|n| n.to_string())
        .unwrap_or_else(|| "signal".to_string());
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let detail = if !stderr.is_empty() {
        stderr
    } else if !stdout.is_empty() {
        stdout
    } else {
        "curl failed".to_string()
    };
    Err(format!("{context} failed with exit code {code}: {detail}"))
}

fn extend_headers(args: &mut Vec<String>, headers: &[String]) {
    for header in headers {
        args.push("-H".to_string());
        args.push(header.clone());
    }
}

fn headers_with_json_accept(headers: &[String]) -> Vec<String> {
    if headers.iter().any(|header| header.starts_with("Accept:")) {
        return headers.to_vec();
    }
    let mut out = headers.to_vec();
    out.push("Accept: application/json".to_string());
    out
}

pub fn get(url: &str, headers: &[String]) -> Result<String, String> {
    let mut args = vec!["-sS".to_string(), "-L".to_string()];
    extend_headers(&mut args, headers);
    args.push(url.to_string());
    run_curl(&args, "http.get()")
}

pub fn post(url: &str, data: &str, headers: &[String]) -> Result<String, String> {
    let mut args = vec![
        "-sS".to_string(),
        "-L".to_string(),
        "-X".to_string(),
        "POST".to_string(),
        "--data".to_string(),
        data.to_string(),
    ];
    extend_headers(&mut args, headers);
    args.push(url.to_string());
    run_curl(&args, "http.post()")
}

pub fn head(url: &str, headers: &[String]) -> Result<String, String> {
    let mut args = vec!["-sS".to_string(), "-L".to_string(), "-I".to_string()];
    extend_headers(&mut args, headers);
    args.push(url.to_string());
    run_curl(&args, "http.head()")
}

pub fn getjson(url: &str, headers: &[String]) -> Result<String, String> {
    get(url, &headers_with_json_accept(headers))
}

#[cfg(test)]
mod tests {
    use super::headers_with_json_accept;

    #[test]
    fn adds_json_accept_header_when_missing() {
        let headers = headers_with_json_accept(&["X-Test: yes".to_string()]);
        assert_eq!(
            headers,
            vec!["X-Test: yes".to_string(), "Accept: application/json".to_string()]
        );
    }

    #[test]
    fn keeps_existing_accept_header() {
        let headers = headers_with_json_accept(&["X-Test: yes".to_string(), "Accept: text/plain".to_string()]);
        assert_eq!(
            headers,
            vec!["X-Test: yes".to_string(), "Accept: text/plain".to_string()]
        );
    }
}
