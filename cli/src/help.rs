//! `suture help`: the agent book, compiled into the binary so the command
//! and the book never differ (appendices AO, AR). The core,
//! docs/agents/core.md, is read whole; each topic, docs/agents/topics, is
//! read when a module uses its forms or an error names it.
//! runtime/tests/guide.rs checks every example in them.

use std::path::Path;

use suture_lang::ast::Source;

const CORE: &str = include_str!("../../docs/agents/core.md");

const TOPICS: [&str; 10] = [
    include_str!("../../docs/agents/topics/01-kinds.md"),
    include_str!("../../docs/agents/topics/02-gate.md"),
    include_str!("../../docs/agents/topics/03-parts.md"),
    include_str!("../../docs/agents/topics/04-rules.md"),
    include_str!("../../docs/agents/topics/05-reactions.md"),
    include_str!("../../docs/agents/topics/06-time.md"),
    include_str!("../../docs/agents/topics/07-scenarios.md"),
    include_str!("../../docs/agents/topics/08-models.md"),
    include_str!("../../docs/agents/topics/09-serve.md"),
    include_str!("../../docs/agents/topics/10-examples.md"),
];

/// A topic's name and what it covers, from its `# ⟨topic⟩: ⟨what⟩` heading.
fn head(text: &str) -> (&str, &str) {
    let h = text.lines().next().unwrap_or("").trim_start_matches("# ");
    h.split_once(": ").unwrap_or((h, ""))
}

fn topic(name: &str) -> Option<&'static str> {
    TOPICS.iter().copied().find(|t| head(t).0 == name)
}

/// The core, a topic, or everything for `all`.
pub fn help(name: Option<&str>) -> Result<String, String> {
    match name {
        None => Ok(CORE.to_string()),
        Some("all") => Ok(std::iter::once(CORE).chain(TOPICS).collect::<Vec<_>>().join("\n")),
        Some(t) => topic(t).map(str::to_string).ok_or_else(|| {
            let names: Vec<&str> = TOPICS.iter().map(|t| head(t).0).collect();
            format!("no topic `{t}`; the topics are {}, and `all`", names.join(", "))
        }),
    }
}

/// `suture help --for ⟨dir or file⟩`: the topics whose forms a module, one
/// of its files or a scenario file uses, in the book's order (appendix AR.2.3).
pub fn help_for(path: &Path) -> Result<String, String> {
    if !path.exists() {
        return Err(format!("{}: no such file or directory", path.display()));
    }
    let names = used(path)?;
    if names.is_empty() {
        return Ok(format!("{} has no Suture yet; `suture help` lists every form\n", path.display()));
    }
    let mut out = format!("{} uses the topics {}; the core (`suture help`) lists every other form\n\n", path.display(), names.join(", "));
    out += &names.iter().filter_map(|n| topic(n)).collect::<Vec<_>>().join("\n");
    Ok(out)
}

/// The topics a path uses. A statement that does not parse is skipped, so
/// a module being written still gets its topics.
fn used(path: &Path) -> Result<Vec<&'static str>, String> {
    let files: Vec<std::path::PathBuf> = if path.is_dir() {
        let mut fs: Vec<_> = std::fs::read_dir(path).map_err(|e| format!("{}: {e}", path.display()))?.flatten().map(|e| e.path()).collect();
        fs.sort();
        fs
    } else {
        vec![path.to_path_buf()]
    };
    let mut src = Source::default();
    let (mut scenarios, mut days) = (false, false);
    for f in &files {
        match f.extension().and_then(|x| x.to_str()) {
            Some("st") => {
                let text = std::fs::read_to_string(f).map_err(|e| format!("{}: {e}", f.display()))?;
                let _ = suture_lang::parse::parse_all(&f.display().to_string(), &text, &mut src);
                days |= text.contains("the days from ");
            }
            Some("scenario") => scenarios = true,
            _ => {}
        }
    }
    let time = days || !src.calendars.is_empty() || src.rules.iter().any(|r| r.period.is_some()) || !src.goals.is_empty() || src.reactions.iter().any(|r| r.outcome.ends_with(suture_lang::ir::PASSES));
    let models = src.decides.iter().any(|d| d.model.is_some()) || src.sources.iter().any(|s| s.model) || !src.allows.is_empty();
    let has = [
        ("kinds", src.module.is_some() || !src.kinds.is_empty()),
        ("gate", !src.roles.is_empty() || !src.admits.is_empty()),
        ("parts", !src.needs.is_empty() || !src.slots.is_empty() || !src.uses.is_empty() || !src.effects.is_empty()),
        ("rules", !src.rules.is_empty() || !src.defines.is_empty() || !src.keeps.is_empty() || !src.applies.is_empty()),
        ("reactions", !src.reactions.is_empty()),
        ("time", time),
        ("scenarios", scenarios),
        ("models", models),
    ];
    Ok(has.into_iter().filter(|(_, on)| *on).map(|(n, _)| n).collect())
}

/// The topic that explains the statement an error is on, by its first
/// word, for `suture check` (appendix AR.2.4).
/// A line that names no statement, such as a whole-module error, gets `all`.
pub fn topic_of_line(line: &str, scenario: bool) -> &'static str {
    if scenario {
        return "scenarios";
    }
    let words: Vec<&str> = line.split_whitespace().collect();
    match words.first().copied().unwrap_or("") {
        "module" | "kind" => "kinds",
        "role" | "admit" => "gate",
        "rule" if line.contains(" for each ") => "time",
        "rule" if line.contains(" taken from each ") => "parts",
        "rule" | "define" | "keep" | "apply" => "rules",
        "desired" | "calendar" => "time",
        "when" if line.contains(" passes") => "time",
        "when" => "reactions",
        "decide" | "source" | "allow" => "models",
        "effect" | "prefer" | "sends" | "marks" | "reports" => "parts",
        w if w.starts_with(char::is_uppercase) => "parts",
        _ => "all",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_core_fits_in_a_context_and_every_topic_is_reachable() {
        // The core is read whole by every agent: keep it under about 2.5k tokens.
        assert!(CORE.len() < 10_000, "the core has grown to {} bytes", CORE.len());
        let names: Vec<&str> = TOPICS.iter().map(|t| head(t).0).collect();
        assert_eq!(names, ["kinds", "gate", "parts", "rules", "reactions", "time", "scenarios", "models", "serve", "examples"]);
        // Every topic the core's form list names exists.
        let forms = &CORE[CORE.find("## forms").unwrap()..CORE.find("## recipes").unwrap()];
        let list = forms.split("```").nth(1).unwrap();
        for line in list.lines().skip(1).filter(|l| !l.is_empty()) {
            let last = line.split_whitespace().last().unwrap();
            assert!(names.contains(&last), "`{line}` names no topic");
        }
        assert!(help(Some("rules")).unwrap().starts_with("# rules:"));
        assert!(help(Some("nope")).unwrap_err().starts_with("no topic `nope`"));
        assert!(help(Some("all")).unwrap().contains("# examples:"));
    }

    #[test]
    fn help_for_a_module_gives_the_topics_it_uses() {
        let dir = std::env::temp_dir().join(format!("suture-help-for-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("loans.st"), "module loans version 1, currency EUR\nrole staff\nkind Loan { due: Time = created + 14 days, overdue: Bool = no }\nwhen a Loan's due passes: it is overdue\nrule broken this does not parse\nadmit lend a new Loan by staff\n").unwrap();
        let text = help_for(&dir).unwrap();
        assert!(text.starts_with(&format!("{} uses the topics kinds, gate, reactions, time;", dir.display())), "{text}");
        assert!(text.contains("# time:") && !text.contains("# models:"));
        std::fs::write(dir.join("loans.scenario"), "scenario \"x\":\n").unwrap();
        assert!(help_for(&dir).unwrap().contains("# scenarios:"));
        assert!(help_for(&dir.join("loans.scenario")).unwrap().contains("uses the topics scenarios;"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn an_error_points_to_the_topic_of_its_statement() {
        assert_eq!(topic_of_line("rule seats: the number of each Class's Bookings is at most 3", false), "rules");
        assert_eq!(topic_of_line("rule daily: for each Member and day, the number of their Bookings is at most 2", false), "time");
        assert_eq!(topic_of_line("when a Loan's due passes: it is overdue", false), "time");
        assert_eq!(topic_of_line("  when a member cancels a Booking: it is cancelled", false), "reactions");
        assert_eq!(topic_of_line("Booking needs Payment", false), "parts");
        assert_eq!(topic_of_line("rule stock: the total copies taken from each Title is at most its stock", false), "parts");
        assert_eq!(topic_of_line("", false), "all");
        assert_eq!(topic_of_line("  expect B1 is paid", true), "scenarios");
    }
}
