// Converting between the Markdown that older notes are written in and the
// HTML that rich notes are stored as.

/// Markdown → HTML, enough for everything a note can contain.
pub fn markdown_to_html(md: &str) -> String {
    let mut out = String::new();
    let mut in_code = false;
    let mut code = String::new();
    let mut list: Option<&str> = None;
    let mut paragraph: Vec<String> = Vec::new();

    fn flush_paragraph(out: &mut String, paragraph: &mut Vec<String>) {
        if paragraph.is_empty() {
            return;
        }
        let text = paragraph.join(" ");
        out.push_str("<p>");
        out.push_str(&inline(&text));
        out.push_str("</p>");
        paragraph.clear();
    }

    fn close_list(out: &mut String, list: &mut Option<&str>) {
        if let Some(kind) = list.take() {
            out.push_str(if kind == "ul" { "</ul>" } else { "</ol>" });
        }
    }

    for line in md.lines() {
        let trimmed = line.trim_end();
        let leading = trimmed.trim_start();

        if leading.starts_with("```") {
            if in_code {
                out.push_str("<pre><code>");
                out.push_str(&escape(&code));
                out.push_str("</code></pre>");
                code.clear();
                in_code = false;
            } else {
                flush_paragraph(&mut out, &mut paragraph);
                close_list(&mut out, &mut list);
                in_code = true;
            }
            continue;
        }
        if in_code {
            code.push_str(line);
            code.push('\n');
            continue;
        }

        if leading.is_empty() {
            flush_paragraph(&mut out, &mut paragraph);
            close_list(&mut out, &mut list);
            continue;
        }

        // Headings
        let hashes = leading.bytes().take_while(|b| *b == b'#').count();
        if (1..=6).contains(&hashes) && leading[hashes..].starts_with(' ') {
            flush_paragraph(&mut out, &mut paragraph);
            close_list(&mut out, &mut list);
            let level = hashes.min(3);
            out.push_str(&format!("<h{level}>{}</h{level}>", inline(leading[hashes + 1..].trim())));
            continue;
        }

        if leading == "---" || leading == "***" {
            flush_paragraph(&mut out, &mut paragraph);
            close_list(&mut out, &mut list);
            out.push_str("<hr>");
            continue;
        }

        // Checklists and bullets
        if let Some(rest) = leading.strip_prefix("- [ ] ").or_else(|| leading.strip_prefix("* [ ] ")) {
            flush_paragraph(&mut out, &mut paragraph);
            if list != Some("ul") {
                close_list(&mut out, &mut list);
                out.push_str("<ul>");
                list = Some("ul");
            }
            out.push_str(&format!(
                "<li><input type=\"checkbox\"> {}</li>",
                inline(rest)
            ));
            continue;
        }
        if let Some(rest) = leading
            .strip_prefix("- [x] ")
            .or_else(|| leading.strip_prefix("- [X] "))
            .or_else(|| leading.strip_prefix("* [x] "))
        {
            flush_paragraph(&mut out, &mut paragraph);
            if list != Some("ul") {
                close_list(&mut out, &mut list);
                out.push_str("<ul>");
                list = Some("ul");
            }
            out.push_str(&format!(
                "<li><input type=\"checkbox\" checked> {}</li>",
                inline(rest)
            ));
            continue;
        }
        if let Some(rest) = leading.strip_prefix("- ").or_else(|| leading.strip_prefix("* ")) {
            flush_paragraph(&mut out, &mut paragraph);
            if list != Some("ul") {
                close_list(&mut out, &mut list);
                out.push_str("<ul>");
                list = Some("ul");
            }
            out.push_str(&format!("<li>{}</li>", inline(rest)));
            continue;
        }

        // Numbered lists
        let digits = leading.bytes().take_while(|b| b.is_ascii_digit()).count();
        if digits > 0 && leading[digits..].starts_with(". ") {
            flush_paragraph(&mut out, &mut paragraph);
            if list != Some("ol") {
                close_list(&mut out, &mut list);
                out.push_str("<ol>");
                list = Some("ol");
            }
            out.push_str(&format!("<li>{}</li>", inline(&leading[digits + 2..])));
            continue;
        }

        // Quotes
        if let Some(rest) = leading.strip_prefix("> ") {
            flush_paragraph(&mut out, &mut paragraph);
            close_list(&mut out, &mut list);
            out.push_str(&format!("<blockquote>{}</blockquote>", inline(rest)));
            continue;
        }

        paragraph.push(leading.to_string());
    }

    flush_paragraph(&mut out, &mut paragraph);
    close_list(&mut out, &mut list);
    if in_code {
        out.push_str("<pre><code>");
        out.push_str(&escape(&code));
        out.push_str("</code></pre>");
    }
    out
}

/// Whether a body still looks like Markdown rather than HTML.
pub fn looks_like_markdown(body: &str) -> bool {
    !body.contains("<p>")
        && !body.contains("<h1>")
        && !body.contains("<ul>")
        && !body.contains("<blockquote>")
        && body.contains(|c: char| c == '#' || c == '*' || c == '`' || c == ']')
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Inline Markdown: code, bold, italic, strike, underline, highlight, colour,
/// links and images.
fn inline(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;

    while !rest.is_empty() {
        if let Some(after) = rest.strip_prefix("`") {
            if let Some(end) = after.find('`') {
                out.push_str(&format!("<code>{}</code>", escape(&after[..end])));
                rest = &after[end + 1..];
                continue;
            }
        }
        if let Some(after) = rest.strip_prefix("**") {
            if let Some(end) = after.find("**") {
                out.push_str(&format!("<strong>{}</strong>", inline(&after[..end])));
                rest = &after[end + 2..];
                continue;
            }
        }
        if let Some(after) = rest.strip_prefix("~~") {
            if let Some(end) = after.find("~~") {
                out.push_str(&format!("<s>{}</s>", inline(&after[..end])));
                rest = &after[end + 2..];
                continue;
            }
        }
        if let Some(after) = rest.strip_prefix('*') {
            if let Some(end) = after.find('*') {
                out.push_str(&format!("<em>{}</em>", inline(&after[..end])));
                rest = &after[end + 1..];
                continue;
            }
        }
        if let Some(after) = rest.strip_prefix("![") {
            if let (Some(mid), Some(end)) = (after.find("]("), after.find(')')) {
                if mid < end {
                    let alt = &after[..mid];
                    let src = &after[mid + 2..end];
                    out.push_str(&format!(
                        "<img src=\"{}\" alt=\"{}\">",
                        escape(src),
                        escape(alt)
                    ));
                    rest = &after[end + 1..];
                    continue;
                }
            }
        }
        if let Some(after) = rest.strip_prefix('[') {
            if let Some(mid) = after.find("](") {
                if let Some(end) = after[mid + 2..].find(')') {
                    let label = &after[..mid];
                    let href = &after[mid + 2..mid + 2 + end];
                    out.push_str(&format!(
                        "<a href=\"{}\">{}</a>",
                        escape(href),
                        inline(label)
                    ));
                    rest = &after[mid + 2 + end + 1..];
                    continue;
                }
            }
        }
        // Already-HTML fragments are passed through untouched.
        if let Some(after) = rest.strip_prefix('<') {
            if let Some(end) = after.find('>') {
                out.push('<');
                out.push_str(&after[..end]);
                out.push('>');
                rest = &after[end + 1..];
                continue;
            }
        }

        let ch = rest.chars().next().unwrap_or(' ');
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(ch),
        }
        rest = &rest[ch.len_utf8()..];
    }

    out
}

/// A plain-text version of an HTML body, for previews and exports.
pub fn html_to_text(html: &str) -> String {
    let mut out = String::new();
    let mut rest = html;
    while !rest.is_empty() {
        if let Some(start) = rest.find('<') {
            out.push_str(&rest[..start]);
            match rest[start..].find('>') {
                Some(end) => rest = &rest[start + end + 1..],
                None => break,
            }
        } else {
            out.push_str(rest);
            break;
        }
    }
    out.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&nbsp;", " ")
}
