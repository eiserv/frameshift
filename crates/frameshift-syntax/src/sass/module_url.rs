//! The URI syntax used by the pinned Sass parser, without resolution.

/// Validates a URI and returns its path, retaining percent escapes until
/// namespace derivation. Dart's URI reader accepts and encodes stray `%` and
/// non-URI characters; it rejects malformed schemes, IP literals, and ports.
pub(super) fn path(url: &str) -> Option<String> {
    let end = url.find(['?', '#']).unwrap_or(url.len());
    let main = &url[..end];
    let first = main.find([':', '/', '\\']);
    let (scheme, rest) = if first.is_some_and(|i| main.as_bytes()[i] == b':') {
        let i = first?;
        let scheme = &main[..i];
        if !scheme.as_bytes().first()?.is_ascii_alphabetic()
            || !scheme
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'+' | b'-' | b'.'))
        {
            return None;
        }
        (scheme, &main[i + 1..])
    } else {
        ("", main)
    };
    if scheme.eq_ignore_ascii_case("data") {
        return data_path(&url[scheme.len() + 1..]);
    }
    let normalized = rest.replace('\\', "/");
    let path = if let Some(authority) = normalized.strip_prefix("//") {
        let end = authority.find('/').unwrap_or(authority.len());
        validate_authority(&authority[..end])?;
        &authority[end..]
    } else {
        &normalized
    };
    Some(path.to_owned())
}

fn validate_authority(authority: &str) -> Option<()> {
    let host_port = if let Some((user, host)) = authority.split_once('@') {
        if user.contains(['[', ']']) || host.contains('@') {
            return None;
        }
        host
    } else {
        authority
    };
    let (host, port) = if host_port.starts_with('[') {
        let end = host_port.find(']')?;
        let rest = &host_port[end + 1..];
        if !rest.is_empty() && !rest.starts_with(':') {
            return None;
        }
        (&host_port[..=end], rest.strip_prefix(':'))
    } else {
        host_port
            .rsplit_once(':')
            .map_or((host_port, None), |(host, port)| (host, Some(port)))
    };
    if let Some(port) = port.filter(|p| !p.is_empty()) {
        // The JS distribution of the pinned compiler uses ECMAScript trim
        // whitespace for numeric ports, which differs at U+0085 and U+FEFF.
        let port = port.trim_matches(|c| matches!(c, '\u{9}'..='\u{d}' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'..='\u{200a}' | '\u{2028}' | '\u{2029}' | '\u{202f}' | '\u{205f}' | '\u{3000}' | '\u{feff}'));
        let digits = port.strip_prefix(['+', '-']).unwrap_or(port);
        let valid = if let Some(hex) = digits
            .strip_prefix("0x")
            .or_else(|| digits.strip_prefix("0X"))
        {
            !hex.is_empty() && hex.bytes().all(|c| c.is_ascii_hexdigit())
        } else {
            !digits.is_empty() && digits.bytes().all(|c| c.is_ascii_digit())
        };
        if !valid {
            return None;
        }
    }
    if let Some(ip) = host.strip_prefix('[').and_then(|h| h.strip_suffix(']')) {
        if let Some(future) = ip.strip_prefix('v') {
            let (version, address) = future.split_once('.')?;
            if version.is_empty()
                || !version.bytes().all(|c| c.is_ascii_hexdigit())
                || address.is_empty()
                || !address
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"-._~!$&'()*+,;=:".contains(&c))
            {
                return None;
            }
        } else {
            let (ip, zone) = ip
                .split_once('%')
                .map_or((ip, None), |(ip, zone)| (ip, Some(zone)));
            ip.parse::<std::net::Ipv6Addr>().ok()?;
            if zone.is_some_and(|z| z.contains('%') && percent_bytes(z, true).is_none()) {
                return None;
            }
        }
    } else if host.contains([':', '[', ']']) {
        return None;
    }
    Some(())
}

fn data_path(rest: &str) -> Option<String> {
    let (header, payload) = rest.split_once(',')?;
    let mut parts = header.split(';');
    let mime = parts.next()?;
    if !mime.is_empty() && mime.bytes().filter(|&c| c == b'/').count() != 1 {
        return None;
    }
    let mut base64 = false;
    let mut params = parts.peekable();
    while let Some(param) = params.next() {
        if param.contains('=') {
            continue;
        }
        if param != "base64" || params.peek().is_some() {
            return None;
        }
        base64 = true;
    }
    let mut path = format!("{header},");
    if base64 {
        let bytes = percent_bytes(payload, true)?;
        let data = bytes.split(|&c| c == b'=').next()?;
        if !data
            .iter()
            .all(|c| c.is_ascii_alphanumeric() || b"+/-_".contains(c))
        {
            return None;
        }
        let padding = bytes.len() - data.len();
        if data.len() % 4 == 1
            || padding > 2
            || !bytes[data.len()..].iter().all(|&c| c == b'=')
            || (padding > 0 && bytes.len() % 4 != 0)
        {
            return None;
        }
        path.extend(bytes.iter().map(|&c| match c {
            b'_' => '/',
            b'-' => '+',
            _ => c as char,
        }));
        if padding == 0 {
            path.extend(std::iter::repeat_n('=', (4 - bytes.len() % 4) % 4));
        }
    } else {
        path.push_str(payload);
    }
    // Data URIs have queries, but their `#` belongs to the path or query.
    path.truncate(path.find('?').unwrap_or(path.len()));
    Some(path)
}

fn percent_bytes(text: &str, strict: bool) -> Option<Vec<u8>> {
    let mut result = Vec::with_capacity(text.len());
    let mut bytes = text.bytes();
    while let Some(c) = bytes.next() {
        if c == b'%' {
            let mut lookahead = bytes.clone();
            let value = lookahead
                .next()
                .and_then(|c| (c as char).to_digit(16))
                .zip(lookahead.next().and_then(|c| (c as char).to_digit(16)));
            if let Some((high, low)) = value {
                result.push((high * 16 + low) as u8);
                bytes = lookahead;
                continue;
            }
            if strict {
                return None;
            }
        }
        result.push(c);
    }
    Some(result)
}

pub(super) fn valid_namespace(path: &str) -> bool {
    let Some(bytes) = percent_bytes(path.rsplit('/').next().unwrap_or(""), false) else {
        return false;
    };
    let Ok(basename) = std::str::from_utf8(&bytes) else {
        return false;
    };
    let name = basename.split('.').next().unwrap_or("");
    identifier(name.strip_prefix('_').unwrap_or(name))
}

fn identifier(name: &str) -> bool {
    let mut chars = name.chars().peekable();
    if chars.peek() == Some(&'-') {
        chars.next();
        if chars.peek() == Some(&'-') {
            chars.next();
            return identifier_body(chars);
        }
    }
    match chars.next() {
        Some('\\') => {
            if !escape(&mut chars) {
                return false;
            }
        }
        Some(c) if c.is_ascii_alphabetic() || c == '_' || c >= '\u{80}' => {}
        _ => return false,
    }
    identifier_body(chars)
}

fn identifier_body(mut chars: std::iter::Peekable<std::str::Chars<'_>>) -> bool {
    while let Some(c) = chars.next() {
        if c == '\\' {
            if !escape(&mut chars) {
                return false;
            }
        } else if !(c.is_ascii_alphanumeric() || c == '-' || c == '_' || c >= '\u{80}') {
            return false;
        }
    }
    true
}

fn escape(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> bool {
    let Some(first) = chars.next() else {
        return false;
    };
    if matches!(first, '\r' | '\n' | '\u{c}') {
        return false;
    }
    if let Some(mut value) = first.to_digit(16) {
        for _ in 1..6 {
            let Some(next) = chars.peek().and_then(|c| c.to_digit(16)) else {
                break;
            };
            chars.next();
            value = value * 16 + next;
        }
        if chars.peek().is_some_and(|&c| super::css_space(c)) {
            chars.next();
        }
        if value > 0x10ffff {
            return false;
        }
    }
    true
}
