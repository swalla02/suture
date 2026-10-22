//! `suture help`: the agent cards, compiled into the binary so the command
//! and its documentation never differ (appendix AO). runtime/tests/guide.rs
//! checks every example in them.

/// The cards, in order. Each starts `# ⟨topic⟩: ⟨what it covers⟩`.
const CARDS: [&str; 11] = [
    include_str!("../../docs/agents/cards/01-start.md"),
    include_str!("../../docs/agents/cards/02-kinds.md"),
    include_str!("../../docs/agents/cards/03-gate.md"),
    include_str!("../../docs/agents/cards/04-rules.md"),
    include_str!("../../docs/agents/cards/05-parts.md"),
    include_str!("../../docs/agents/cards/06-time.md"),
    include_str!("../../docs/agents/cards/07-recipes.md"),
    include_str!("../../docs/agents/cards/08-scenarios.md"),
    include_str!("../../docs/agents/cards/09-models.md"),
    include_str!("../../docs/agents/cards/10-api.md"),
    include_str!("../../docs/agents/cards/11-limits.md"),
];

/// A card's topic and summary, from its first line.
fn head(card: &str) -> (&str, &str) {
    let first = card.lines().next().unwrap_or("").trim_start_matches("# ");
    first.split_once(": ").unwrap_or((first, ""))
}

/// The card for `topic`, every card for `all`, or the list of topics.
pub fn help(topic: Option<&str>) -> Result<String, String> {
    let list = || {
        let mut out = String::from("suture help ⟨topic⟩ prints one card; `suture help all` prints every card.\n\n");
        for c in CARDS {
            let (t, what) = head(c);
            out += &format!("  {t:<10} {what}\n");
        }
        out
    };
    match topic {
        None => Ok(list()),
        Some("all") => Ok(CARDS.join("\n")),
        Some(t) => CARDS.iter().find(|c| head(c).0 == t).map(|c| c.to_string()).ok_or_else(|| format!("no card `{t}`\n\n{}", list())),
    }
}

/// The topics, for a client that lists them.
pub fn topics() -> Vec<(&'static str, &'static str)> {
    CARDS.iter().map(|c| head(c)).collect()
}
