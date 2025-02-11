pub mod config;
pub mod template;

pub fn generate_list<S: AsRef<str>>(base: &str, word: &str, els: &[S]) -> String {
    if !els.is_empty() {
        let mut requires = String::new();

        if !base.is_empty() {
            requires.push_str(base);
            requires.push(' ');
        }

        for (i, r) in els.iter().enumerate() {
            if i == 0 {
                requires.push_str(word);
                requires.push(' ');
            } else if i == els.len() - 1 {
                requires.push_str(" and ");
            } else {
                requires.push_str(", ");
            };

            requires.push('`');
            requires.push_str(r.as_ref());
            requires.push('`');
        }
        requires.push('.');

        requires
    } else {
        base.to_string()
    }
}
