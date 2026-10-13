//! Takes a word as it stands in a sentence back to the forms a dictionary
//! may list it under: 食べさせられた to 食べる. This is Yomitan's
//! `LanguageTransformer` (ext/js/language/language-transformer.js) with its
//! Japanese rules, which `tools/yomitan-dump.mjs` writes out as data.
//!
//! The rules know no words. Each says that text ending one way may come from
//! text ending another way, given what the text was taken for so far, so what
//! comes out is every form the text could have, most of them no word at all.
//! The dictionary settles which are: a form counts when it is listed there as
//! a word of the kind the rules took it for.

use std::collections::HashMap;

use serde_json::Value;

struct Rule {
    /// the rule is for this word alone, not for every text that ends in it
    whole: bool,
    inflected: String,
    deinflected: String,
    /// what the text must have been taken for, for the rule to apply
    from: u32,
    /// what the rule takes the text it makes for
    to: u32,
}

struct Transform {
    id: String,
    rules: Vec<Rule>,
}

/// A rule applied to a text: the transform, the rule in it, and the text.
type Step = (usize, usize, String);

struct Form {
    text: String,
    conditions: u32,
    /// how the text was come by, the last step first
    steps: Vec<Step>,
}

/// A form the text looked up may have.
#[derive(Debug, PartialEq, Clone)]
pub struct Deinflected {
    pub text: String,
    /// What the rules took the form for, as flags: verb of some kind,
    /// adjective. Nothing for the text as it was written.
    pub conditions: u32,
    /// the names of the inflections undone, the last one undone first
    pub rules: Vec<String>,
}

pub struct Transformer {
    transforms: Vec<Transform>,
    /// the flags of the kinds of word a dictionary marks its entries with
    parts: HashMap<String, u32>,
}

/// No condition past this one has a flag to itself, as in Yomitan.
const FLAGS: u32 = 32;

fn texts(value: &Value) -> Vec<&str> {
    value.as_array().into_iter().flatten().filter_map(Value::as_str).collect()
}

/// The flags of every condition. One that stands for others has theirs, so
/// it has to wait until they have been given flags.
fn flags(conditions: &[Value]) -> Result<HashMap<String, u32>, String> {
    let mut known: HashMap<String, u32> = HashMap::new();
    let mut next = 0;
    let mut waiting: Vec<&Value> = conditions.iter().collect();
    while !waiting.is_empty() {
        let mut still = Vec::new();
        for condition in &waiting {
            let name = condition["name"].as_str().ok_or("a condition without a name")?;
            let flags = if condition["sub"].is_null() {
                if next >= FLAGS {
                    return Err("too many conditions".to_string());
                }
                next += 1;
                1 << (next - 1)
            } else {
                match together(&known, &texts(&condition["sub"])) {
                    Some(flags) => flags,
                    None => {
                        still.push(*condition);
                        continue;
                    }
                }
            };
            known.insert(name.to_string(), flags);
        }
        if still.len() == waiting.len() {
            return Err("conditions that stand for each other".to_string());
        }
        waiting = still;
    }
    Ok(known)
}

/// The flags of all the conditions named; `None` if one is not known.
fn together(known: &HashMap<String, u32>, names: &[&str]) -> Option<u32> {
    names.iter().try_fold(0, |flags, name| known.get(*name).map(|more| flags | more))
}

impl Transformer {
    /// Reads the conditions and transforms as the dump writes them.
    pub fn new(data: &Value) -> Result<Transformer, String> {
        let conditions = data["conditions"].as_array().ok_or("no conditions")?;
        let known = flags(conditions)?;
        let mut transforms = Vec::new();
        for transform in data["transforms"].as_array().ok_or("no transforms")? {
            let id = transform["id"].as_str().ok_or("a transform without a name")?.to_string();
            let mut rules = Vec::new();
            for rule in transform["rules"].as_array().ok_or("a transform without rules")? {
                let text = |field: &str| rule[field].as_str().map(str::to_string).ok_or(format!("a rule of {id} without {field}"));
                let named = |field: &str| together(&known, &texts(&rule[field])).ok_or(format!("a rule of {id} names a condition that is not there"));
                rules.push(Rule { whole: rule["type"] == "wholeWord", inflected: text("inflected")?, deinflected: text("deinflected")?, from: named("in")?, to: named("out")? });
            }
            transforms.push(Transform { id, rules });
        }
        let listed = conditions.iter().filter(|c| c["dictionary"] == true).filter_map(|c| c["name"].as_str());
        let parts = listed.map(|name| (name.to_string(), known[name])).collect();
        Ok(Transformer { transforms, parts })
    }

    /// The flags of the kinds of word a dictionary entry is marked as, the
    /// way it writes them: `v5 vt`. Marks the rules know nothing of add none.
    pub fn parts(&self, marks: &str) -> u32 {
        marks.split(' ').filter_map(|mark| self.parts.get(mark)).fold(0, |flags, more| flags | more)
    }

    /// Every form the text may be an inflection of, the text itself first,
    /// then those fewer rules away before those more.
    pub fn transform(&self, text: &str) -> Vec<Deinflected> {
        let mut forms = vec![Form { text: text.to_string(), conditions: 0, steps: Vec::new() }];
        let mut at = 0;
        while at < forms.len() {
            for (t, transform) in self.transforms.iter().enumerate() {
                for (r, rule) in transform.rules.iter().enumerate() {
                    let form = &forms[at];
                    if !matches(form.conditions, rule.from) {
                        continue;
                    }
                    let fits = if rule.whole { form.text == rule.inflected } else { form.text.ends_with(&rule.inflected) };
                    // rules can lead in a circle; one is not applied to the same text twice on a way
                    if !fits || form.steps.iter().any(|step| step.0 == t && step.1 == r && step.2 == form.text) {
                        continue;
                    }
                    let kept = if rule.whole { "" } else { &form.text[..form.text.len() - rule.inflected.len()] };
                    let mut steps = vec![(t, r, form.text.clone())];
                    steps.extend(form.steps.iter().cloned());
                    forms.push(Form { text: format!("{kept}{}", rule.deinflected), conditions: rule.to, steps });
                }
            }
            at += 1;
        }
        let named = |form: Form| Deinflected { text: form.text, conditions: form.conditions, rules: form.steps.iter().map(|step| self.transforms[step.0].id.clone()).collect() };
        forms.into_iter().map(named).collect()
    }
}

/// Whether a text taken for `conditions` can be what `others` asks for or
/// names: they share a condition, or the text was not taken for anything.
pub fn matches(conditions: u32, others: u32) -> bool {
    conditions == 0 || conditions & others != 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dict::tests::{data, sum, vectors};

    #[test]
    fn the_same_forms_as_yomitan() {
        let transformer = Transformer::new(&data()).unwrap();
        let vectors = vectors();
        let words = vectors["transforms"].as_array().unwrap();
        assert!(words.len() > 80);
        for word in words {
            let text = word["text"].as_str().unwrap();
            let wanted: Vec<Deinflected> = word["forms"].as_array().unwrap().iter().map(|form| Deinflected { text: form["text"].as_str().unwrap().to_string(), conditions: form["conditions"].as_u64().unwrap() as u32, rules: texts(&form["rules"]).into_iter().map(str::to_string).collect() }).collect();
            assert_eq!(transformer.transform(text), wanted, "forms of {text}");
        }
    }

    /// Thousands of texts, among them one for every rule there is; of each
    /// only a number for all the forms Yomitan found.
    #[test]
    fn the_same_forms_as_yomitan_at_large() {
        let transformer = Transformer::new(&data()).unwrap();
        let vectors = vectors();
        let texts = vectors["formed"].as_array().unwrap();
        assert!(texts.len() > 8000);
        for row in texts {
            let text = row[0].as_str().unwrap();
            let forms = transformer.transform(text);
            let lines: Vec<String> = forms.iter().map(|form| format!("{}\t{}\t{}", form.text, form.conditions, form.rules.join(","))).collect();
            assert_eq!(forms.len() as u64, row[1].as_u64().unwrap(), "forms of {text}: {lines:?}");
            assert_eq!(sum(&lines.join("\n")), row[2].as_str().unwrap(), "forms of {text}: {lines:?}");
        }
    }

    #[test]
    fn the_same_flags_as_yomitan() {
        let transformer = Transformer::new(&data()).unwrap();
        for part in vectors()["parts"].as_array().unwrap() {
            let marks = part["rules"].as_str().unwrap();
            assert_eq!(transformer.parts(marks) as u64, part["flags"].as_u64().unwrap(), "flags of {marks}");
        }
    }

    #[test]
    fn dictionary_forms() {
        let transformer = Transformer::new(&data()).unwrap();
        // found as the kind of word the rules took it for, and not as another
        let listed = |text: &str, form: &str, marks: &str| transformer.transform(text).iter().any(|f| f.text == form && matches(f.conditions, transformer.parts(marks)));
        assert!(listed("食べさせられた", "食べる", "v1"));
        assert!(!listed("食べさせられた", "食べる", "v5"));
        assert!(listed("行った", "行く", "v5"));
        assert!(listed("高くなかった", "高い", "adj-i"));
        assert!(listed("来なかった", "来る", "vk"));
        assert!(listed("した", "する", "vs"));
        // a word that is not inflected is found whatever it is marked as
        assert!(listed("猫", "猫", "n"));
        assert!(listed("猫", "猫", ""));
    }

    #[test]
    fn conditions_that_cannot_be_read() {
        let circle = serde_json::json!({"conditions": [{"name": "a", "dictionary": false, "sub": ["b"]}, {"name": "b", "dictionary": false, "sub": ["a"]}], "transforms": []});
        assert!(Transformer::new(&circle).is_err());
        let unknown = serde_json::json!({"conditions": [{"name": "a", "dictionary": true, "sub": null}], "transforms": [{"id": "t", "rules": [{"type": "suffix", "inflected": "x", "deinflected": "y", "in": ["z"], "out": ["a"]}]}]});
        assert!(Transformer::new(&unknown).is_err());
    }
}
