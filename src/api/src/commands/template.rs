//! Reply templates: `{name}` is replaced by the variable's value. Unknown names stay as
//! they are, so a typo shows up in chat instead of silently vanishing.

pub fn render(template: &str, vars: &[(&str, String)]) -> String {
    let mut out = String::with_capacity(template.len() + 32);
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        match after.find('}') {
            Some(end) => {
                let name = &after[..end];
                match vars.iter().find(|(k, _)| *k == name) {
                    Some((_, v)) => out.push_str(v),
                    None => {
                        out.push('{');
                        out.push_str(name);
                        out.push('}');
                    }
                }
                rest = &after[end + 1..];
            }
            None => {
                out.push_str(&rest[start..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaces_known_names_only() {
        let vars = [("user", "Nat".to_string()), ("title", "歌".to_string())];
        assert_eq!(render("{user} 點了『{title}』", &vars), "Nat 點了『歌』");
        assert_eq!(render("{oops} {user}", &vars), "{oops} Nat");
        assert_eq!(render("no vars", &vars), "no vars");
        assert_eq!(render("dangling {user", &vars), "dangling {user");
    }
}
