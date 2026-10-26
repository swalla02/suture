//! P1 and P3: the prompt LLM reads the policy and the case. P1 answers, with
//! RuleArena's own prompt (`auto_test.py`, zero-shot, the tables as
//! tables); P3 writes a Python program for the case, which runs in a
//! container with no network, a read-only file system and a time limit.

use std::process::Command;

/// RuleArena's system prompt, word for word.
pub const SYSTEM: &str = "You are a helpful assistant at American Airlines. You are given the information of a passenger, his / her items, his / her special needs, and the policies of American Airlines. You should compute the total cost (including the flight ticket fee, checked bag fees, cost of special needs) according to the policies for the passenger.";

pub fn p1(rules: &str, case: &str) -> String {
    format!(
        "\nThe policies of American Airlines are as follows:\n\n{rules}\n\n{case} Compute the total cost for him step by step (don't omit any bag) and end your response with \"The total cost is $xxx.\" (xxx is a number)\nYour response:\n"
    )
}

pub fn p3(rules: &str, case: &str) -> String {
    format!(
        "\nThe policies of American Airlines are as follows:\n\n{rules}\n\n{case}\n\nWrite a Python 3 program that computes the total cost for this passenger according to the policies: the flight ticket and every bag fee. The program takes no input and prints only the total cost, as a whole number of dollars. Answer with the program alone, in one ```python block.\n"
    )
}

/// The number after the last "The total cost is $", as RuleArena reads it,
/// but tolerant of cents and of markdown.
pub fn p1_answer(text: &str) -> Option<i64> {
    let text = text.replace("**", "");
    let at = text.rfind("The total cost is")?;
    let rest = &text[at..];
    let rest = &rest[rest.find('$')? + 1..];
    let num: String = rest.chars().take_while(|c| c.is_ascii_digit() || *c == ',' || *c == '.').collect::<String>().replace(',', "");
    let v: f64 = num.trim_end_matches('.').parse().ok()?;
    (v.fract() == 0.0).then_some(v as i64)
}

/// The program in the last fenced block.
pub fn p3_program(text: &str) -> Option<String> {
    let parts: Vec<&str> = text.split("```").collect();
    // Fenced blocks are the odd-numbered parts.
    let block = parts.iter().skip(1).step_by(2).last()?;
    let block = block.strip_prefix("python").or_else(|| block.strip_prefix("py")).unwrap_or(block);
    Some(block.trim_start_matches(['\r', '\n']).to_string())
}

/// The last number the program printed.
pub fn p3_answer(stdout: &str) -> Option<i64> {
    let token = stdout.split_whitespace().last()?;
    let v: f64 = token.trim_start_matches('$').replace(',', "").trim_end_matches('.').parse().ok()?;
    (v.fract() == 0.0).then_some(v as i64)
}

pub const IMAGE: &str = "python:3.12.7-slim";

/// Runs `program` in the container: no network, nothing writable, no
/// privileges, 256 MiB, 10 seconds. Its standard output, or why none.
pub fn sandboxed(program: &str, scratch: &std::path::Path) -> Result<String, String> {
    std::fs::create_dir_all(scratch).map_err(|e| e.to_string())?;
    std::fs::write(scratch.join("case.py"), program).map_err(|e| e.to_string())?;
    let out = Command::new("timeout")
        .args(["30", "docker", "run", "--rm", "--network", "none", "--read-only", "--cap-drop", "ALL", "--security-opt", "no-new-privileges"])
        .args(["--pids-limit", "64", "--memory", "256m", "--cpus", "1", "--user", "65534:65534", "-v"])
        .arg(format!("{}:/w:ro", scratch.display()))
        .args([IMAGE, "timeout", "10", "python3", "-I", "/w/case.py"])
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(format!("exit {:?}: {}", out.status.code(), err.lines().last().unwrap_or("")));
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_answer_as_rulearena_asks_for_it() {
        assert_eq!(p1_answer("... so\n**The total cost is $1,198.**"), Some(1198));
        assert_eq!(p1_answer("The total cost is $10. Wait. The total cost is $1198.00."), Some(1198));
        assert_eq!(p1_answer("The total cost is $11.50."), None);
        assert_eq!(p1_answer("no answer"), None);
    }

    #[test]
    fn takes_the_last_program_and_its_last_number() {
        let t = "Here:\n```python\nprint(1)\n```\nor better\n```python\nprint(1198)\n```\n";
        assert_eq!(p3_program(t).as_deref(), Some("print(1198)\n"));
        assert_eq!(p3_answer("Total: $1,198\n"), Some(1198));
        assert_eq!(p3_answer("1198.0\n"), Some(1198));
        assert_eq!(p3_answer(""), None);
    }

    #[test]
    fn the_prompt_is_rulearenas() {
        let p = p1("RULES", "Jim is ...");
        assert!(p.starts_with("\nThe policies of American Airlines are as follows:\n\nRULES\n\nJim is ... Compute the total cost"));
    }
}
