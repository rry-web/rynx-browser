//! glasses.conf: glasses mode's screen and layout (GLASS-04 in TESTING.md)

use rynx::glasses::{DEFAULT_TEXT, Layout};

#[test]
fn the_written_file_is_the_default_layout() {
    let (layout, problems) = Layout::parse(DEFAULT_TEXT);
    assert_eq!(problems, Vec::<String>::new());
    assert_eq!(layout, Layout::default());
    assert_eq!(layout.screen, Some((80, 30)));
}

#[test]
fn sizes_in_characters_pixels_or_the_window() {
    let (layout, problems) = Layout::parse("screen = 48x16\ntitle_row = off\nmargin = 0\ncolour = on");
    assert!(problems.is_empty(), "{problems:?}");
    assert_eq!(layout.screen, Some((48, 16)));
    assert!(!layout.title_row && layout.status_row && layout.colour);
    assert_eq!(layout.margin, 0);
    // A display's pixels and the font's: 640x350 with 8x16 is 80 columns and 21 rows
    assert_eq!(Layout::parse("screen = 640x350 pixels").0.screen, Some((80, 21)));
    assert_eq!(Layout::parse("font = 8x20\nscreen = 304×256 pixels").0.screen, Some((38, 12)));
    assert_eq!(Layout::parse("screen = window").0.screen, None);
}

#[test]
fn problems_are_said_and_the_defaults_kept() {
    let (layout, problems) = Layout::parse("screen = big\nmargin = 99\nsparkles = on\ntitle_row = maybe\nnonsense");
    assert_eq!(layout, Layout::default());
    assert_eq!(problems.len(), 5, "{problems:?}");
    assert!(problems[0].contains("line 1") && problems[2].contains("unknown setting \"sparkles\""), "{problems:?}");
    // Too small a screen is made big enough, and margins leave ten columns of page
    let (layout, problems) = Layout::parse("screen = 4x1\nmargin = 10");
    assert_eq!(layout.screen, Some((10, 3)));
    assert_eq!(layout.margin, 0);
    assert_eq!(problems.len(), 1, "{problems:?}");
}

#[test]
fn the_file_is_written_when_missing() {
    let dir = std::env::temp_dir().join(format!("rynx-glasses-layout-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(Layout::load_or_create(&dir).0, Layout::default());
    assert_eq!(std::fs::read_to_string(dir.join("glasses.conf")).unwrap(), DEFAULT_TEXT);
    std::fs::write(dir.join("glasses.conf"), "screen = 40x12\n").unwrap();
    assert_eq!(Layout::load_or_create(&dir).0.screen, Some((40, 12)));
    assert!(Layout::load(&dir.join("missing.conf")).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_example_layouts_have_no_problems() {
    let folder = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("glasses/layouts");
    let mut screens = Vec::new();
    for entry in std::fs::read_dir(folder).unwrap() {
        let path = entry.unwrap().path();
        let (layout, problems) = Layout::load(&path).unwrap();
        assert!(problems.is_empty(), "{}: {problems:?}", path.display());
        screens.push((path.file_stem().unwrap().to_string_lossy().into_owned(), layout.screen));
    }
    screens.sort();
    assert!(screens.contains(&("640x200".into(), Some((80, 12)))), "{screens:?}");
    assert!(screens.contains(&("304x256".into(), Some((38, 16)))), "{screens:?}");
    assert!(screens.contains(&("large-font".into(), Some((53, 20)))), "{screens:?}");
}
