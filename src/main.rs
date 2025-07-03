use std::path::PathBuf;
use std::{collections::HashMap, fs};

use anyhow::{Context, Result, bail};
use clap::Parser;
use ratatui::DefaultTerminal;
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::widgets::{
    Block, Borders, List, ListItem, ListState, Paragraph, StatefulWidget, Widget,
};
use serde::Deserialize;

#[derive(Parser)]
#[command(name = "dbdrill")]
#[command(about = "A PostgreSQL database drilling tool")]
#[command(version)]
struct Args {
    /// PostgreSQL database connection string (DSN)
    #[arg(
        help = "PostgreSQL database connection string (e.g., postgres://user:password@host:port/database)"
    )]
    db_dsn: String,

    /// Path to the TOML resources file
    #[arg(help = "Path to the TOML file containing resources configuration")]
    resources_file: PathBuf,
}

#[derive(Debug, Deserialize)]
struct SearchParam {
    name: String,
}

#[derive(Debug, Deserialize)]
struct Search {
    query: String,
    params: Vec<SearchParam>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum LinkSearchParam {
    Name(String),
    JsonDeref { json_deref: Vec<String> },
}

#[derive(Debug, Deserialize)]
struct Link {
    kind: String,
    search: String,
    search_params: Vec<LinkSearchParam>,
}

#[derive(Debug, Deserialize)]
struct Resource {
    name: String,
    search: HashMap<String, Search>,
    links: Option<HashMap<String, Link>>,
}

fn validate_resource_link(resources: &HashMap<String, Resource>, link: &Link) -> Result<()> {
    let Some(target_resource) = resources.get(&link.kind) else {
        bail!("link references a non existing resource {}", &link.kind);
    };

    let Some(target_search) = target_resource.search.get(&link.search) else {
        bail!(
            "referenced resource {} has no search named {}",
            &link.kind,
            &link.search
        );
    };

    if target_search.params.len() != link.search_params.len() {
        bail!(
            "referenced search {} has {} params but link specifies {}",
            &link.search,
            target_search.params.len(),
            link.search_params.len()
        );
    }

    Ok(())
}

fn validate_resource_links(
    resources: &HashMap<String, Resource>,
    links: &HashMap<String, Link>,
) -> Result<()> {
    for (link_name, link) in links {
        validate_resource_link(resources, link)
            .with_context(|| format!("error validating link {link_name}"))?;
    }
    Ok(())
}

fn validate_resources(resources: &HashMap<String, Resource>) -> Result<()> {
    for (resource_id, resource) in resources {
        if let Some(links) = &resource.links {
            validate_resource_links(resources, links)
                .with_context(|| format!("error validating {resource_id}.links"))?;
        }
    }
    Ok(())
}

struct ResourceListItem {
    id: String,
    name: String,
}

struct RouteListResources<'a> {
    resources: &'a HashMap<String, Resource>,
    resource_list_model: Vec<ResourceListItem>,
    resource_list_state: ListState,
}

impl<'a> RouteListResources<'a> {
    fn new(resources: &'a HashMap<String, Resource>) -> Self {
        let mut resource_list_model: Vec<ResourceListItem> = resources
            .iter()
            .map(|(id, resource)| ResourceListItem {
                id: id.clone(),
                name: resource.name.clone(),
            })
            .collect();
        resource_list_model.sort_by_cached_key(|item| item.name.to_lowercase());

        let mut resource_list_state = ListState::default();
        if !resource_list_model.is_empty() {
            resource_list_state.select(Some(0));
        }

        Self {
            resources,
            resource_list_model,
            resource_list_state,
        }
    }
}

impl<'a> Widget for &mut RouteListResources<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let block = Block::new()
            .title(Line::raw("Resources").centered())
            .borders(Borders::TOP);
        let items: Vec<ListItem> = self
            .resource_list_model
            .iter()
            .map(|r| ListItem::new(r.name.clone()))
            .collect();
        let list = List::new(items)
            .block(block)
            .highlight_symbol("> ")
            .highlight_spacing(ratatui::widgets::HighlightSpacing::Always);
        StatefulWidget::render(list, area, buf, &mut self.resource_list_state);
    }
}

enum AppRoute<'a> {
    ListResources(RouteListResources<'a>),
}

enum Action {}

struct App<'a> {
    resources: &'a HashMap<String, Resource>,
    should_exit: bool,
    route: AppRoute<'a>,
}

impl<'a> App<'a> {
    fn new(resources: &'a HashMap<String, Resource>) -> Self {
        Self {
            resources,
            should_exit: false,
            route: AppRoute::ListResources(RouteListResources::new(resources)),
        }
    }

    pub fn run(mut self, mut terminal: DefaultTerminal) -> Result<()> {
        while !self.should_exit {
            terminal.draw(|frame| frame.render_widget(&mut self, frame.area()))?;
            if let Event::Key(key) = event::read()? {
                self.handle_key(key);
            };
        }
        Ok(())
    }

    fn handle_key(&mut self, key: KeyEvent) {
        if key.kind != KeyEventKind::Press {
            return;
        }
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => self.should_exit = true,
            _ => {}
        }
        match &mut self.route {
            AppRoute::ListResources(route) => {
                App::handle_keypress_list_resources(route, key);
            }
        }
    }

    fn handle_keypress_list_resources(route: &mut RouteListResources, key: KeyEvent) {
        match key.code {
            KeyCode::Char('k') | KeyCode::Up => route.resource_list_state.select_previous(),
            KeyCode::Char('j') | KeyCode::Down => route.resource_list_state.select_next(),
            _ => {}
        }
    }

    fn render_header(area: Rect, buf: &mut Buffer) {
        Paragraph::new("dbdrill")
            .bold()
            .centered()
            .render(area, buf);
    }

    fn render_route(&mut self, area: Rect, buf: &mut Buffer) {
        match &mut self.route {
            AppRoute::ListResources(route) => {
                route.render(area, buf);
            }
        }
    }
}

impl<'a> Widget for &mut App<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let [header_area, main_area, _footer_area] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Fill(1),
            Constraint::Length(1),
        ])
        .areas(area);

        App::render_header(header_area, buf);
        self.render_route(main_area, buf);
    }
}

fn main() -> Result<()> {
    let args = Args::parse();

    println!("Database DSN: {}", args.db_dsn);
    println!("Resources file: {}", args.resources_file.display());

    let resources: HashMap<String, Resource> = toml::from_str(
        &fs::read_to_string(&args.resources_file).context("error opening resources file")?,
    )
    .context("error parsing resources files")?;

    validate_resources(&resources).context("error validating resources")?;

    let terminal = ratatui::init();
    let app_result = App::new(&resources).run(terminal);
    ratatui::restore();

    app_result
}
