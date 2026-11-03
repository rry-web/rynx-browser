//! Recipes: most recipe sites describe the recipe for search engines as schema.org data (JSON-LD
//! in a <script>), whatever the page around it looks like: its name, ingredients, steps, and
//! sometimes a picture for each step. rynx reads that (never runs it) to show the recipe a step
//! at a time, for cooking with your hands busy, or on glasses.

use super::*;
use serde_json::Value;

/// The most of each kept, and the longest text: a page can say anything here
const MAX_ITEMS: usize = 200;
const MAX_TEXT: usize = 2000;
/// Larger scripts aren't read (a recipe is a few kilobytes; some sites put megabytes of data in
/// scripts of this type)
const MAX_SCRIPT_BYTES: usize = 2 * 1024 * 1024;
/// How deep in the data a recipe is looked for, and its sections' steps
const MAX_DEPTH: usize = 8;

/// A recipe as the page describes it
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Recipe {
    pub name: String,
    pub description: Option<String>,
    /// "Serves 4", "Makes 12"
    pub yields: Option<String>,
    /// Preparation, cooking and total time, written for reading ("1 hr 20 min")
    pub prep_time: Option<String>,
    pub cook_time: Option<String>,
    pub total_time: Option<String>,
    /// The recipe's picture's address, as written
    pub image: Option<String>,
    pub ingredients: Vec<String>,
    pub steps: Vec<RecipeStep>,
}

/// One step: what to do, the part of the recipe it's in ("For the sauce"), and its picture
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RecipeStep {
    pub text: String,
    pub section: Option<String>,
    pub image: Option<String>,
}

/// The page's recipe, if it describes one with steps
pub(super) fn find_recipe(document: &Html) -> Option<Recipe> {
    static SCRIPT: OnceLock<Selector> = OnceLock::new();
    let selector = SCRIPT.get_or_init(|| Selector::parse("script[type]").expect("valid selector"));
    document
        .select(selector)
        .filter(|s| s.value().attr("type").is_some_and(|t| t.trim().eq_ignore_ascii_case("application/ld+json")))
        .find_map(|script| {
            let text: String = script.text().collect();
            if text.len() > MAX_SCRIPT_BYTES {
                return None;
            }
            let data: Value = serde_json::from_str(text.trim()).ok()?;
            recipe_in(&data, 0).and_then(read_recipe)
        })
}

/// The first object of type Recipe in the data: it, in a list, or in an @graph
fn recipe_in(data: &Value, depth: usize) -> Option<&Value> {
    if depth > MAX_DEPTH {
        return None;
    }
    match data {
        Value::Array(items) => items.iter().find_map(|item| recipe_in(item, depth + 1)),
        Value::Object(object) => {
            if is_type(data, "Recipe") {
                return Some(data);
            }
            object.get("@graph").and_then(|graph| recipe_in(graph, depth + 1))
        }
        _ => None,
    }
}

/// Whether an object's @type is `kind` (it may be a list: ["Recipe", "NewsArticle"])
fn is_type(data: &Value, kind: &str) -> bool {
    let matches = |t: &Value| t.as_str().is_some_and(|t| t.trim_start_matches("schema:") == kind);
    match data.get("@type") {
        Some(Value::Array(types)) => types.iter().any(matches),
        Some(t) => matches(t),
        None => false,
    }
}

fn read_recipe(data: &Value) -> Option<Recipe> {
    let mut steps = Vec::new();
    if let Some(instructions) = data.get("recipeInstructions") {
        read_steps(instructions, None, &mut steps, 0);
    }
    if steps.is_empty() {
        return None;
    }
    let ingredients = match data.get("recipeIngredient").or_else(|| data.get("ingredients")) {
        Some(Value::Array(items)) => items.iter().filter_map(text_of).take(MAX_ITEMS).collect(),
        Some(item) => text_of(item).into_iter().collect(),
        None => Vec::new(),
    };
    Some(Recipe {
        name: data.get("name").and_then(text_of).unwrap_or_else(|| "Recipe".to_string()),
        description: data.get("description").and_then(text_of),
        yields: data.get("recipeYield").and_then(yields),
        prep_time: data.get("prepTime").and_then(|t| t.as_str()).and_then(duration),
        cook_time: data.get("cookTime").and_then(|t| t.as_str()).and_then(duration),
        total_time: data.get("totalTime").and_then(|t| t.as_str()).and_then(duration),
        image: data.get("image").and_then(image_of),
        ingredients,
        steps,
    })
}

/// The steps in recipeInstructions: one text, a list of texts, steps (HowToStep), or sections
/// (HowToSection) of steps
fn read_steps(data: &Value, section: Option<&str>, steps: &mut Vec<RecipeStep>, depth: usize) {
    if depth > MAX_DEPTH || steps.len() >= MAX_ITEMS {
        return;
    }
    match data {
        Value::Array(items) => {
            for item in items {
                read_steps(item, section, steps, depth + 1);
            }
        }
        // One text: its lines are the steps (a list in one string, numbered or not)
        Value::String(_) => {
            let Some(text) = step_text(data) else { return };
            let lines: Vec<&str> = data.as_str().unwrap_or_default().lines().filter(|l| !l.trim().is_empty()).collect();
            if lines.len() > 1 {
                for line in lines {
                    read_steps(&Value::String(line.to_string()), section, steps, depth + 1);
                }
            } else {
                steps.push(RecipeStep { text, section: section.map(str::to_string), image: None });
            }
        }
        Value::Object(_) if is_type(data, "HowToSection") || data.get("itemListElement").is_some() => {
            let name = data.get("name").and_then(text_of);
            if let Some(items) = data.get("itemListElement") {
                read_steps(items, name.as_deref().or(section), steps, depth + 1);
            }
        }
        Value::Object(_) => {
            // A step's text, or its name where that's all it has
            let Some(text) = data.get("text").and_then(step_text).or_else(|| data.get("name").and_then(step_text))
            else {
                return;
            };
            let image = data.get("image").and_then(image_of);
            steps.push(RecipeStep { text, section: section.map(str::to_string), image });
        }
        _ => {}
    }
}

/// Text from the data: HTML in it (some sites put tags and entities there) read as text,
/// whitespace collapsed, and cut to a sensible length; None if empty
fn text_of(data: &Value) -> Option<String> {
    let raw = match data {
        Value::String(s) => s.as_str(),
        Value::Number(n) => return Some(n.to_string()),
        _ => return None,
    };
    let text = if raw.contains('<') || raw.contains('&') {
        Html::parse_fragment(raw).root_element().text().collect::<String>()
    } else {
        raw.to_string()
    };
    let joined = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let text = sanitize_for_terminal(&joined);
    let text: String = text.chars().take(MAX_TEXT).collect();
    (!text.is_empty()).then_some(text)
}

/// A step's text without a leading step number ("1." or "Step 2:"), which the view shows itself
fn step_text(data: &Value) -> Option<String> {
    let text = text_of(data)?;
    let stripped = strip_step_number(&text);
    (!stripped.is_empty()).then(|| stripped.to_string())
}

fn strip_step_number(text: &str) -> &str {
    let rest = text.strip_prefix("Step ").or_else(|| text.strip_prefix("STEP ")).unwrap_or(text);
    let digits = rest.len() - rest.trim_start_matches(|c: char| c.is_ascii_digit()).len();
    if digits == 0 || digits > 2 {
        return text;
    }
    match rest[digits..].strip_prefix(['.', ':', ')']) {
        Some(after) if after.starts_with(' ') => after.trim_start(),
        _ if rest.len() < text.len() => rest[digits..].trim_start(),
        _ => text,
    }
}

/// An image's address: a string, an ImageObject (its url or contentUrl), or the first of a list
fn image_of(data: &Value) -> Option<String> {
    match data {
        Value::String(s) => Some(s.trim().to_string()).filter(|s| !s.is_empty() && !s.starts_with("data:")),
        Value::Array(items) => items.iter().find_map(image_of),
        Value::Object(object) => object.get("url").or_else(|| object.get("contentUrl")).and_then(image_of),
        _ => None,
    }
}

/// What the recipe makes: "4", "Serves 4", or a list of the ways it says so (the longest reads
/// best: ["4", "4 servings"])
fn yields(data: &Value) -> Option<String> {
    match data {
        Value::Array(items) => items.iter().filter_map(text_of).max_by_key(|s| s.len()),
        other => text_of(other),
    }
}

/// An ISO 8601 duration ("PT1H20M", "P0DT0H45M") written for reading ("1 hr 20 min"); None if
/// it's empty, zero, or not a duration
fn duration(iso: &str) -> Option<String> {
    let rest = iso.trim().strip_prefix('P')?;
    let mut parts = Vec::new();
    let mut number = String::new();
    let mut in_time = false;
    for c in rest.chars() {
        match c {
            'T' => in_time = true,
            '0'..='9' | '.' => number.push(c),
            _ => {
                let value: f64 = std::mem::take(&mut number).parse().ok()?;
                let unit = match (c, in_time) {
                    ('D', false) => "day",
                    ('H', true) => "hr",
                    ('M', true) => "min",
                    ('S', true) => "sec",
                    ('W', false) => "week",
                    _ => return None,
                };
                if value > 0.0 {
                    let value = if value.fract() == 0.0 { format!("{}", value as u64) } else { format!("{value}") };
                    let unit = if matches!(unit, "day" | "week") && value != "1" {
                        format!("{unit}s")
                    } else {
                        unit.to_string()
                    };
                    parts.push(format!("{value} {unit}"));
                }
            }
        }
    }
    (number.is_empty() && !parts.is_empty()).then(|| parts.join(" "))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn recipe(json: &str) -> Option<Recipe> {
        find_recipe(&Html::parse_document(&format!(
            "<html><head><script type='application/ld+json'>{json}</script></head><body>Hi</body></html>"
        )))
    }

    #[test]
    fn a_recipe_with_steps_is_read() {
        // As BBC Good Food writes it
        let found = recipe(
            r#"{"@context":"https://schema.org","@type":"Recipe","name":"Easy pancakes",
                "image":[{"@type":"ImageObject","url":"https://img.example/pancakes.jpg"}],
                "recipeYield":"Makes 12","prepTime":"PT10M","cookTime":"PT20M","totalTime":"PT1H30M",
                "recipeIngredient":["100g plain flour","2 large eggs"],
                "recipeInstructions":[{"@type":"HowToStep","text":"Whisk to a smooth batter."},
                                      {"@type":"HowToStep","text":"Set aside for 30 mins.","image":"https://img.example/rest.jpg"}]}"#,
        )
        .unwrap();
        assert_eq!(found.name, "Easy pancakes");
        assert_eq!(found.image.as_deref(), Some("https://img.example/pancakes.jpg"));
        assert_eq!(found.yields.as_deref(), Some("Makes 12"));
        assert_eq!(found.prep_time.as_deref(), Some("10 min"));
        assert_eq!(found.total_time.as_deref(), Some("1 hr 30 min"));
        assert_eq!(found.ingredients, ["100g plain flour", "2 large eggs"]);
        assert_eq!(found.steps.len(), 2);
        assert_eq!(found.steps[1].text, "Set aside for 30 mins.");
        assert_eq!(found.steps[1].image.as_deref(), Some("https://img.example/rest.jpg"));
    }

    #[test]
    fn recipes_in_graphs_with_sections_are_read() {
        // As WordPress recipe plugins write it: in an @graph, steps in named sections, with HTML
        let found = recipe(
            r#"{"@context":"https://schema.org","@graph":[{"@type":"WebPage","name":"Blog"},
                {"@type":["Recipe","NewsArticle"],"name":"Pie &amp; mash",
                 "recipeYield":["4","4 servings"],
                 "recipeInstructions":[
                   {"@type":"HowToSection","name":"For the pastry","itemListElement":[
                     {"@type":"HowToStep","text":"<p>Rub the <b>butter</b> into the flour.</p>"}]},
                   {"@type":"HowToSection","name":"To finish","itemListElement":[
                     {"@type":"HowToStep","name":"Bake","text":"Step 2: Bake for 40 minutes."}]}]}]}"#,
        )
        .unwrap();
        assert_eq!(found.name, "Pie & mash");
        assert_eq!(found.yields.as_deref(), Some("4 servings"));
        assert_eq!(found.steps[0].text, "Rub the butter into the flour.");
        assert_eq!(found.steps[0].section.as_deref(), Some("For the pastry"));
        assert_eq!(found.steps[1].text, "Bake for 40 minutes.");
        assert_eq!(found.steps[1].section.as_deref(), Some("To finish"));
    }

    #[test]
    fn steps_in_one_text_are_split_by_line() {
        let found = recipe(
            r#"{"@type":"Recipe","name":"Tea","recipeInstructions":"1. Boil the water.\n2. Pour it on the tea.\n\n3. Wait."}"#,
        )
        .unwrap();
        let texts: Vec<_> = found.steps.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(texts, ["Boil the water.", "Pour it on the tea.", "Wait."]);
    }

    #[test]
    fn what_isnt_a_recipe_with_steps_is_ignored() {
        assert!(recipe(r#"{"@type":"Article","name":"News"}"#).is_none());
        assert!(recipe(r#"{"@type":"Recipe","name":"No steps","recipeIngredient":["salt"]}"#).is_none());
        assert!(recipe("not json at all").is_none());
        assert!(recipe(r#"[[[[[[[[[[[[{"@type":"Recipe","recipeInstructions":"Too deep"}]]]]]]]]]]]]"#).is_none());
        // Escapes for the terminal are taken out of what's shown
        let found = recipe(r#"{"@type":"Recipe","name":"Bad\u001b[2J name","recipeInstructions":"Stir."}"#).unwrap();
        assert!(!found.name.contains('\u{1b}'), "{:?}", found.name);
    }

    #[test]
    fn durations_are_written_for_reading() {
        assert_eq!(duration("PT45M").as_deref(), Some("45 min"));
        assert_eq!(duration("P0DT1H0M").as_deref(), Some("1 hr"));
        assert_eq!(duration("P2D").as_deref(), Some("2 days"));
        assert_eq!(duration("PT0M"), None);
        assert_eq!(duration(""), None);
        assert_eq!(duration("20 minutes"), None);
        assert_eq!(duration("PT20"), None);
    }
}
