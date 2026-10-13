const NAMED: [(&str, &str); 12] = [
    ("&lt;", "<"),
    ("&gt;", ">"),
    ("&quot;", "\""),
    ("&apos;", "'"),
    ("&nbsp;", " "),
    ("&ndash;", "-"),
    ("&mdash;", "-"),
    ("&rsquo;", "'"),
    ("&lsquo;", "'"),
    ("&ldquo;", "\""),
    ("&rdquo;", "\""),
    ("&amp;", "&"),
];

const BREAKERS: [&str; 12] = [
    "</p>", "<br>", "<br/>", "<br />", "</li>", "</div>", "</h1>", "</h2>", "</h3>", "</h4>",
    "</tr>", "</ul>",
];

const MAX_ENTITY: usize = 12;

pub fn plain(raw: &str) -> String {
    let once = strip_tags(&decode(raw));
    let twice = strip_tags(&decode(&once));
    collapse(&twice)
}

fn decode(raw: &str) -> String {
    let mut out = String::new();
    let mut rest = raw;
    loop {
        let Some(start) = rest.find('&') else {
            out.push_str(rest);
            return out;
        };
        out.push_str(&rest[..start]);
        rest = &rest[start..];
        let head: String = rest.chars().take(MAX_ENTITY).collect();
        let Some(stop) = head.find(';') else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let entity = &head[..=stop];
        let replaced = swap(entity);
        match replaced {
            Swap::Text(text) => {
                out.push_str(&text);
                rest = &rest[entity.len()..];
            }
            Swap::Keep => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
}

enum Swap {
    Text(String),
    Keep,
}

fn swap(entity: &str) -> Swap {
    for pair in NAMED {
        if entity.eq_ignore_ascii_case(pair.0) {
            return Swap::Text(pair.1.to_string());
        }
    }
    numeric(entity)
}

fn numeric(entity: &str) -> Swap {
    let inner = entity.trim_start_matches('&').trim_end_matches(';');
    let Some(digits) = inner.strip_prefix('#') else {
        return Swap::Keep;
    };
    let Some(hex) = digits.strip_prefix('x') else {
        return from_radix(entity, digits, 10);
    };
    from_radix(entity, hex, 16)
}

fn from_radix(entity: &str, digits: &str, radix: u32) -> Swap {
    let code = match u32::from_str_radix(digits, radix) {
        Ok(code) => code,
        Err(source) => {
            tracing::debug!(error = %source, entity = entity, "undecodable numeric entity kept raw");
            return Swap::Keep;
        }
    };
    let Some(found) = char::from_u32(code) else {
        return Swap::Keep;
    };
    Swap::Text(found.to_string())
}

fn strip_tags(raw: &str) -> String {
    let mut marked = raw.to_string();
    for breaker in BREAKERS {
        marked = marked.replace(breaker, "\n");
    }
    let mut out = String::new();
    let mut depth: usize = 0;
    for ch in marked.chars() {
        if ch == '<' {
            depth += 1;
            continue;
        }
        if ch == '>' {
            if depth > 0 {
                depth -= 1;
            }
            continue;
        }
        if depth == 0 {
            out.push(ch);
        }
    }
    out
}

fn collapse(raw: &str) -> String {
    let mut lines: Vec<String> = Vec::new();
    for line in raw.lines() {
        let trimmed = line.split_whitespace().collect::<Vec<&str>>().join(" ");
        if trimmed.is_empty() {
            continue;
        }
        lines.push(trimmed);
    }
    lines.join("\n")
}
