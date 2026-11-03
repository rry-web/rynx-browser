//! Recipe mode: a page's recipe (its schema.org data) a step at a time, with the steps' pictures,
//! and the demo recipe built in.

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use rynx::app::{App, MenuKind, RECIPE_DEMO_PAGE};
use rynx::event_handler::{handle_key_event, handle_network_event};
use rynx::network::{NetworkResponse, PageKind};
use rynx::settings::Settings;
use wiremock::matchers::path;
use wiremock::{Mock, MockServer, ResponseTemplate};

fn test_app() -> App {
    let (tx, rx) = tokio::sync::mpsc::channel(10);
    let mut app = App::with_settings(tx, rx, Settings::default()).unwrap();
    app.clipboard = None;
    app.render_in_background = false;
    app.terminal_width = 80;
    app.terminal_height = 30;
    app
}

fn show(app: &mut App, url: &str, html: &str) {
    let id = app.current_tab().id;
    app.current_tab().url_input = url.into();
    let resp = NetworkResponse::Success(id, "t".into(), html.into(), url.into(), PageKind::Html);
    handle_network_event(app, resp, 80).unwrap();
}

fn press(app: &mut App, code: KeyCode) {
    let key =
        KeyEvent { code, modifiers: KeyModifiers::empty(), kind: KeyEventKind::Press, state: KeyEventState::empty() };
    handle_key_event(app, key, 80, 30).unwrap();
}

/// Handle messages until `count` images have arrived
async fn images_arrive(app: &mut App, count: usize) {
    let mut seen = 0;
    while seen < count {
        let resp = tokio::time::timeout(std::time::Duration::from_secs(20), app.rx.recv()).await.unwrap().unwrap();
        seen += usize::from(matches!(resp, NetworkResponse::ImageLoaded(..)));
        handle_network_event(app, resp, 80).unwrap();
    }
}

fn screen(app: &App) -> String {
    let mut terminal = Terminal::new(TestBackend::new(80, 30)).unwrap();
    terminal.draw(|f| rynx::ui::ui(f, app)).unwrap();
    let buffer = terminal.backend().buffer().clone();
    let width = buffer.area.width as usize;
    let symbols: Vec<&str> = buffer.content().iter().map(|c| c.symbol()).collect();
    symbols.chunks(width).map(|row| row.concat()).collect::<Vec<_>>().join("\n")
}

fn status(app: &mut App) -> String {
    app.current_tab().status_message.clone()
}

/// A recipe page as sites write them, its step pictures on `pictures`
fn recipe_page(pictures: &str) -> String {
    format!(
        r#"<html><head><title>Toast</title><script type="application/ld+json">
        {{"@context":"https://schema.org","@type":"Recipe","name":"Toast","recipeYield":"Serves 1",
          "recipeIngredient":["1 slice of bread","Butter"],
          "recipeInstructions":[
            {{"@type":"HowToStep","text":"Toast the bread until golden.","image":"{pictures}/toasting.png"}},
            {{"@type":"HowToStep","text":"Spread the butter while it's hot.","image":"{pictures}/butter.png"}},
            {{"@type":"HowToStep","text":"Eat it."}}]}}
        </script></head><body><h1>Toast</h1><p>Our family's toast, with a long story.</p></body></html>"#
    )
}

fn png() -> Vec<u8> {
    let image = image::RgbImage::from_pixel(40, 20, image::Rgb([200, 120, 40]));
    let mut data = std::io::Cursor::new(Vec::new());
    image.write_to(&mut data, image::ImageFormat::Png).unwrap();
    data.into_inner()
}

async fn picture_site() -> MockServer {
    let server = MockServer::start().await;
    for name in ["/toasting.png", "/butter.png"] {
        Mock::given(path(name))
            .respond_with(ResponseTemplate::new(200).set_body_raw(png(), "image/png"))
            .mount(&server)
            .await;
    }
    server
}

async fn requests(server: &MockServer, of: &str) -> usize {
    server.received_requests().await.unwrap().iter().filter(|r| r.url.path() == of).count()
}

#[tokio::test]
async fn the_demo_recipe_is_cooked_a_step_at_a_time() {
    let mut app = test_app();
    app.follow_link(RECIPE_DEMO_PAGE.to_string());
    assert!(status(&mut app).contains("has a recipe"), "{}", status(&mut app));

    press(&mut app, KeyCode::Char('c'));
    let shown = screen(&app);
    assert!(shown.contains("Fluffy pancakes · Ingredients (7 steps)"), "{shown}");
    assert!(shown.contains("Makes 8 · Prep 10 min · Cook 15 min · Total 25 min"), "{shown}");
    assert!(shown.contains("• 200g plain flour"), "{shown}");
    // The recipe's picture, and the first step's, ready for it; built in, so no network
    images_arrive(&mut app, 2).await;
    assert!(!screen(&app).contains("Loading the picture"));

    press(&mut app, KeyCode::Char('n'));
    let shown = screen(&app);
    assert!(shown.contains("Step 1 of 7"), "{shown}");
    assert!(shown.contains("Put the flour, baking powder"), "{shown}");
    assert!(!shown.contains("200g plain flour"), "only the step: {shown}");

    press(&mut app, KeyCode::Char('5'));
    assert!(screen(&app).contains("Step 5 of 7"));
    press(&mut app, KeyCode::Char('p'));
    assert!(screen(&app).contains("Step 4 of 7"));
    press(&mut app, KeyCode::Char('G'));
    assert!(screen(&app).contains("Step 7 of 7"));
    press(&mut app, KeyCode::Char('n'));
    assert_eq!(status(&mut app), "That's the last step");
    press(&mut app, KeyCode::Char('i'));
    assert!(screen(&app).contains("Ingredients (7 steps)"));
    press(&mut app, KeyCode::Char('9'));
    assert!(status(&mut app).contains("There's no step 9: this recipe has 7"));

    press(&mut app, KeyCode::Esc);
    assert!(app.recipe_view.is_none());
    assert!(screen(&app).contains("A demo of rynx's recipe mode"), "the page again");
}

#[tokio::test]
async fn a_remotes_four_buttons_step_through_a_recipe() {
    // Next link, previous link, open and back: what glasses' buttons or a remote are set up as
    let mut app = test_app();
    app.follow_link(RECIPE_DEMO_PAGE.to_string());
    press(&mut app, KeyCode::Char('c'));
    press(&mut app, KeyCode::Tab);
    assert!(screen(&app).contains("Step 1 of 7"));
    press(&mut app, KeyCode::Enter);
    assert!(screen(&app).contains("Step 2 of 7"));
    press(&mut app, KeyCode::BackTab);
    assert!(screen(&app).contains("Step 1 of 7"));
    press(&mut app, KeyCode::Backspace);
    assert!(app.recipe_view.is_none(), "back closes it, rather than leaving the page");
    assert_eq!(app.current_tab().url_input, RECIPE_DEMO_PAGE);
}

#[tokio::test]
async fn a_sites_step_pictures_load_as_the_steps_are_shown() {
    let server = picture_site().await;
    let mut app = test_app();
    show(&mut app, &format!("{}/toast", server.uri()), &recipe_page(""));
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    assert_eq!(requests(&server, "/toasting.png").await, 0, "nothing loads until the recipe is shown");

    // The ingredients card has no picture; the first step's is fetched ready for it
    press(&mut app, KeyCode::Char('c'));
    assert!(screen(&app).contains("Serves 1"));
    images_arrive(&mut app, 1).await;
    assert_eq!(requests(&server, "/toasting.png").await, 1);
    assert_eq!(requests(&server, "/butter.png").await, 0);

    press(&mut app, KeyCode::Char('n'));
    images_arrive(&mut app, 1).await;
    let shown = screen(&app);
    assert!(shown.contains("Toast the bread until golden."), "{shown}");
    assert!(!shown.contains("[Loading the picture"), "{shown}");
    assert_eq!(requests(&server, "/butter.png").await, 1, "the next step's, ready");

    // A step with no picture is its text alone
    press(&mut app, KeyCode::Char('3'));
    assert!(screen(&app).contains("Eat it."));
}

#[tokio::test]
async fn a_public_pages_recipe_cant_load_pictures_from_the_local_network() {
    let server = picture_site().await;
    let mut app = test_app();
    show(&mut app, "https://evil.example/toast", &recipe_page(&server.uri()));
    press(&mut app, KeyCode::Char('c'));
    press(&mut app, KeyCode::Char('n'));
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    assert_eq!(requests(&server, "/toasting.png").await, 0);
    assert!(screen(&app).contains("Toast the bread until golden."));
}

#[tokio::test]
async fn pages_without_a_recipe_say_so() {
    let mut app = test_app();
    show(&mut app, "https://example.org/", "<h1>News</h1><p>No cooking here.</p>");
    assert!(!status(&mut app).contains("recipe"));
    press(&mut app, KeyCode::Char('c'));
    assert!(app.recipe_view.is_none());
    assert_eq!(status(&mut app), "This page has no recipe rynx can read");
}

#[tokio::test]
async fn the_menu_offers_the_recipe_first() {
    let mut app = test_app();
    show(&mut app, "https://example.org/toast", &format!("{}<a href='/more'>More</a>", recipe_page("")));
    // With a link selected the menu is fullest; it still fits one digit
    press(&mut app, KeyCode::Tab);
    app.show_menu(MenuKind::Main, 28);
    let menu = app.current_tab().menu.as_ref().unwrap();
    assert_eq!(menu.items[0].label, "Recipe, step by step");
    assert!(menu.items.len() <= 9, "{}", menu.items.len());
    press(&mut app, KeyCode::Char('1'));
    assert!(app.recipe_view.is_some());
}
