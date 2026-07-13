use serde::Deserialize;
use tauri::menu::{AboutMetadata, Menu, MenuItem, MenuItemBuilder, MenuItemKind, SubmenuBuilder};
use tauri::{AppHandle, Emitter, Runtime};

pub const MENU_COMMAND_EVENT: &str = "cepa://menu-command";

const OPEN_FOLDER_ID: &str = "cepa.open-folder";
const RESCAN_ID: &str = "cepa.rescan";
const SEARCH_ID: &str = "cepa.search";
const PARENT_ID: &str = "cepa.parent";

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopMenuAvailability {
    pub can_choose_directory: bool,
    pub can_rescan: bool,
    pub can_search: bool,
    pub can_navigate_up: bool,
}

pub fn build<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<Menu<R>> {
    let package = app.package_info();
    let config = app.config();
    let about = AboutMetadata {
        name: Some(package.name.clone()),
        version: Some(package.version.to_string()),
        copyright: config.bundle.copyright.clone(),
        authors: config
            .bundle
            .publisher
            .clone()
            .map(|publisher| vec![publisher]),
        ..Default::default()
    };

    let open_folder = MenuItemBuilder::with_id(OPEN_FOLDER_ID, "Open Folder…")
        .accelerator("CmdOrCtrl+O")
        .build(app)?;
    let rescan = MenuItemBuilder::with_id(RESCAN_ID, "Scan Again")
        .accelerator("CmdOrCtrl+R")
        .enabled(false)
        .build(app)?;
    let search = MenuItemBuilder::with_id(SEARCH_ID, "Search This Folder…")
        .accelerator("CmdOrCtrl+F")
        .enabled(false)
        .build(app)?;
    let parent = MenuItemBuilder::with_id(PARENT_ID, "Go to Parent Folder")
        .accelerator("Alt+Left")
        .enabled(false)
        .build(app)?;

    let mut menu = tauri::menu::MenuBuilder::new(app);

    #[cfg(target_os = "macos")]
    {
        let application = SubmenuBuilder::new(app, package.name.clone())
            .about(Some(about.clone()))
            .separator()
            .services()
            .separator()
            .hide()
            .hide_others()
            .show_all()
            .separator()
            .quit()
            .build()?;
        menu = menu.item(&application);
    }

    let file = SubmenuBuilder::new(app, "File")
        .item(&open_folder)
        .item(&rescan)
        .separator()
        .close_window();
    #[cfg(not(target_os = "macos"))]
    let file = file.quit();

    let edit = SubmenuBuilder::new(app, "Edit")
        .undo()
        .redo()
        .separator()
        .cut()
        .copy()
        .paste()
        .select_all()
        .build()?;

    let view = SubmenuBuilder::new(app, "View").item(&search).item(&parent);
    #[cfg(target_os = "macos")]
    let view = view.separator().fullscreen();

    let window = SubmenuBuilder::new(app, "Window").minimize().maximize();
    #[cfg(target_os = "macos")]
    let window = window.separator().bring_all_to_front();

    let help = SubmenuBuilder::new(app, "Help");
    #[cfg(not(target_os = "macos"))]
    let help = help.about(Some(about));

    menu = menu
        .item(&file.build()?)
        .item(&edit)
        .item(&view.build()?)
        .item(&window.build()?)
        .item(&help.build()?);
    menu.build()
}

pub fn emit_command<R: Runtime>(app: &AppHandle<R>, menu_id: &str) {
    if let Some(command) = command_for_menu_id(menu_id) {
        let _ = app.emit(MENU_COMMAND_EVENT, command);
    }
}

pub fn set_availability<R: Runtime>(
    app: &AppHandle<R>,
    availability: DesktopMenuAvailability,
) -> Result<(), String> {
    set_enabled(app, OPEN_FOLDER_ID, availability.can_choose_directory)?;
    set_enabled(app, RESCAN_ID, availability.can_rescan)?;
    set_enabled(app, SEARCH_ID, availability.can_search)?;
    set_enabled(app, PARENT_ID, availability.can_navigate_up)
}

fn command_for_menu_id(menu_id: &str) -> Option<&'static str> {
    match menu_id {
        OPEN_FOLDER_ID => Some("chooseDirectory"),
        RESCAN_ID => Some("rescan"),
        SEARCH_ID => Some("search"),
        PARENT_ID => Some("navigateUp"),
        _ => None,
    }
}

fn set_enabled<R: Runtime>(app: &AppHandle<R>, id: &str, enabled: bool) -> Result<(), String> {
    menu_item(app, id)?
        .set_enabled(enabled)
        .map_err(|error| format!("Could not update the application menu: {error}"))
}

fn menu_item<R: Runtime>(app: &AppHandle<R>, id: &str) -> Result<MenuItem<R>, String> {
    let menu = app
        .menu()
        .ok_or_else(|| "The application menu is unavailable.".to_string())?;
    let item = menu
        .items()
        .map_err(|error| format!("Could not read the application menu: {error}"))?
        .into_iter()
        .find_map(|item| item.as_submenu().and_then(|submenu| submenu.get(id)))
        .ok_or_else(|| format!("The application menu item {id} is unavailable."))?;
    let item = match item {
        MenuItemKind::MenuItem(item) => item,
        _ => {
            return Err(format!(
                "The application menu item {id} has the wrong type."
            ));
        }
    };
    Ok(item)
}

#[cfg(test)]
mod tests {
    use super::{DesktopMenuAvailability, command_for_menu_id};

    #[test]
    fn custom_menu_ids_map_only_to_supported_frontend_commands() {
        assert_eq!(
            command_for_menu_id("cepa.open-folder"),
            Some("chooseDirectory")
        );
        assert_eq!(command_for_menu_id("cepa.rescan"), Some("rescan"));
        assert_eq!(command_for_menu_id("cepa.search"), Some("search"));
        assert_eq!(command_for_menu_id("cepa.parent"), Some("navigateUp"));
        assert_eq!(command_for_menu_id("quit"), None);
    }

    #[test]
    fn availability_wire_contract_uses_camel_case_fields() {
        let availability: DesktopMenuAvailability = serde_json::from_value(serde_json::json!({
            "canChooseDirectory": true,
            "canRescan": false,
            "canSearch": true,
            "canNavigateUp": false,
        }))
        .expect("deserialize desktop menu availability");
        assert!(availability.can_choose_directory);
        assert!(!availability.can_rescan);
        assert!(availability.can_search);
        assert!(!availability.can_navigate_up);
    }
}
