//! The menu (m): what it lists for a link, a field or the page, and choosing from it (GLASS-05)

use rynx::app::{Menu, MenuAction, MenuKind, MenuResult};
use rynx::models::LinkRegion;

fn region(url: &str, control: Option<usize>) -> LinkRegion {
    LinkRegion { url: url.into(), control, image: None, line_index: 0, x_start: 0, x_end: 4 }
}

fn labels(menu: &Menu) -> Vec<&str> {
    menu.items.iter().map(|i| i.label).collect()
}

#[test]
fn what_it_lists_depends_on_what_is_selected() {
    let link = Menu::new(MenuKind::Main, Some(&region("https://example.org/", None)), false);
    assert_eq!(link.title, "Link: https://example.org/");
    assert_eq!(link.items[0].action, MenuAction::Run("open"));
    assert_eq!(labels(&link).last(), Some(&"Tabs..."));
    let field = Menu::new(MenuKind::Main, Some(&region("", Some(0))), false);
    assert_eq!(field.title, "Field");
    assert_eq!(field.items[0].label, "Use the field");
    let page = Menu::new(MenuKind::Main, None, true);
    assert_eq!(page.items[0].action, MenuAction::Run("hints"));
    assert_eq!(page.items.last().unwrap().action, MenuAction::Talk, "talk when a voice controller can listen");
    // Every item is an action keys.conf knows, or another menu
    for kind in [MenuKind::Main, MenuKind::Page, MenuKind::Tabs] {
        for item in Menu::new(kind, None, true).items {
            if let MenuAction::Run(action) = item.action {
                assert!(rynx::keys::action_key(action).is_some(), "{action}");
            }
        }
        assert!(Menu::new(kind, None, true).items.len() <= 9, "chosen with one digit");
    }
}

#[test]
fn chosen_by_number_or_by_stepping() {
    let mut menu = Menu::new(MenuKind::Tabs, None, false);
    assert_eq!(menu.digit('2'), MenuResult::Chosen(menu.items[1]));
    assert_eq!(menu.digit('0'), MenuResult::Waiting, "no item 0");
    assert_eq!(menu.digit('9'), MenuResult::Waiting, "no item 9");
    assert_eq!(menu.selected, 1, "item 2, chosen last");
    menu.step(false);
    menu.step(false);
    assert_eq!(menu.selected, menu.items.len() - 1, "back from the first is the last");
    menu.step(true);
    assert_eq!(menu.selected, 0);
    assert_eq!(menu.choose(4).map(|i| i.label), Some("Close the tab"));
    assert_eq!(menu.choose(0), None);
}
