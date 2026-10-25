//! The debug menu bar and the windows behind it.
//!
//! A provider is a live Bevy resource that implements [`ImguiMenu`]: usually
//! a service, but any resource will do. It declares its entries (each a
//! category, a name and a stable ID) and draws each open one on every UI
//! pass. The manager keeps only metadata, open flags and a function that
//! finds the provider in the world: never a reference to it or a copy.
//!
//! Registration lasts as long as its owner wants: a service registers in
//! `init` and unregisters in `shutdown`, with the handle `register` returned.

use std::any::{type_name, TypeId};
use std::collections::BTreeMap;
use std::fmt;

use bevy::prelude::*;
use bevy_egui::egui;
use service::{AppLayer, Service};

/// One entry a provider offers: a window under a category's dropdown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MenuSpec {
    /// Stable within the provider; passed back to `show_menu`.
    pub id: &'static str,
    /// The dropdown it is listed under.
    pub category: &'static str,
    /// What the dropdown and the window title say.
    pub name: &'static str,
}

impl MenuSpec {
    pub const fn new(id: &'static str, category: &'static str, name: &'static str) -> Self {
        MenuSpec { id, category, name }
    }
}

/// The entry `show_menu` is asked to draw.
#[derive(Clone, Copy, Debug)]
pub struct MenuId {
    provider: &'static str,
    spec: &'static MenuSpec,
}

impl MenuId {
    pub fn id(&self) -> &'static str {
        self.spec.id
    }

    pub fn name(&self) -> &'static str {
        self.spec.name
    }

    pub fn category(&self) -> &'static str {
        self.spec.category
    }

    /// The window's egui identity: stable across frames and unique per
    /// provider and entry, whatever the title says.
    pub fn egui_id(&self) -> egui::Id {
        egui::Id::new(("debug_ui", self.provider, self.spec.id))
    }

    /// A window titled with the entry's name and identified by
    /// [`MenuId::egui_id`]. Bind its close button with `.open(open)`.
    pub fn window<'a>(&self) -> egui::Window<'a> {
        egui::Window::new(self.spec.name).id(self.egui_id())
    }
}

/// A resource that offers debug windows.
///
/// `show_menu` runs on every UI pass for each open entry, including while its
/// window is collapsed (it may skip expensive work then: `Window::show`
/// returns `inner: None`). It creates its own window, binds its close control
/// to `open`, and must be repeatable: a pass may run more than once a frame.
/// It reads the world freely but changes nothing that matters to the
/// simulation itself: actions go to the [`UiOutbox`], which applies them
/// once after drawing. While it runs the provider is `self` and is not in
/// `world`.
pub trait ImguiMenu: Resource {
    fn menus() -> &'static [MenuSpec];

    fn show_menu(&mut self, id: MenuId, root: &mut egui::Ui, open: &mut bool, world: &mut World);
}

/// Names one registration. A handle from an earlier registration of the
/// same provider removes nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MenuHandle {
    provider: TypeId,
    generation: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RegisterError {
    /// `ImguiManager` isn't registered (or has stopped).
    NoManager,
    DuplicateProvider(&'static str),
    DuplicateEntry { provider: &'static str, id: &'static str },
    /// Two entries would show the same text in the same dropdown.
    DuplicateName { category: &'static str, name: &'static str },
    EmptyLabel { provider: &'static str, id: &'static str },
}

impl fmt::Display for RegisterError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            RegisterError::NoManager => write!(f, "the debug UI manager isn't running; register ImguiManager before its providers"),
            RegisterError::DuplicateProvider(p) => write!(f, "debug menu provider {p} is registered twice"),
            RegisterError::DuplicateEntry { provider, id } => write!(f, "debug menu provider {provider} declares entry {id:?} twice"),
            RegisterError::DuplicateName { category, name } => write!(f, "two debug menu entries are named {category} / {name}"),
            RegisterError::EmptyLabel { provider, id } => write!(f, "debug menu entry {id:?} of {provider} needs an ID, category and name"),
        }
    }
}

impl std::error::Error for RegisterError {}

/// Draws one open entry of a provider; `false` if the provider is gone.
type ShowFn = fn(&mut World, MenuId, &mut egui::Ui, &mut bool) -> bool;

struct Entry {
    spec: &'static MenuSpec,
    open: bool,
}

struct Provider {
    id: TypeId,
    name: &'static str,
    generation: u64,
    entries: Vec<Entry>,
    show: ShowFn,
    exists: fn(&World) -> bool,
}

/// Every debug menu entry, by provider. An App-layer service: register it
/// before its providers so it outlives them.
#[derive(Resource, Default)]
pub struct ImguiManager {
    providers: Vec<Provider>,
    generation: u64,
}

impl Service for ImguiManager {
    type Layer = AppLayer;
    type Deps = ();

    fn init(_world: &mut World) -> Self {
        ImguiManager::default()
    }
}

/// One dropdown of the menu bar, as drawn: entries sorted by name.
#[derive(Debug, PartialEq, Eq)]
pub struct Category {
    pub name: &'static str,
    pub entries: Vec<(&'static str, MenuHandle, &'static str)>,
}

impl ImguiManager {
    /// Registers provider `R`'s entries. Call it from the owner's `init` (a
    /// service registering itself isn't in the world yet; that's fine) and
    /// keep the handle for [`ImguiManager::unregister`].
    ///
    /// Registrations whose provider has left the world without
    /// unregistering are dropped first.
    pub fn register<R: ImguiMenu>(world: &mut World) -> Result<MenuHandle, RegisterError> {
        prune(world, Some(TypeId::of::<R>()));
        let mut manager = world.get_resource_mut::<ImguiManager>().ok_or(RegisterError::NoManager)?;
        manager.add::<R>()
    }

    /// Removes a registration and forgets which of its windows were open.
    /// Returns whether the handle named the current one.
    pub fn unregister(world: &mut World, handle: MenuHandle) -> bool {
        world.get_resource_mut::<ImguiManager>().is_some_and(|mut manager| manager.remove(handle))
    }

    /// Adds `R`'s entries without looking at the world.
    pub fn add<R: ImguiMenu>(&mut self) -> Result<MenuHandle, RegisterError> {
        let id = TypeId::of::<R>();
        let name = type_name::<R>();
        if self.providers.iter().any(|p| p.id == id) {
            return Err(RegisterError::DuplicateProvider(name));
        }
        let specs = R::menus();
        for (index, spec) in specs.iter().enumerate() {
            if spec.id.is_empty() || spec.category.trim().is_empty() || spec.name.trim().is_empty() {
                return Err(RegisterError::EmptyLabel { provider: name, id: spec.id });
            }
            if specs[..index].iter().any(|s| s.id == spec.id) {
                return Err(RegisterError::DuplicateEntry { provider: name, id: spec.id });
            }
            let taken = specs[..index].iter().chain(self.providers.iter().flat_map(|p| p.entries.iter().map(|e| e.spec)));
            if taken.into_iter().any(|s| s.category == spec.category && s.name == spec.name) {
                return Err(RegisterError::DuplicateName { category: spec.category, name: spec.name });
            }
        }
        self.generation += 1;
        let generation = self.generation;
        self.providers.push(Provider {
            id,
            name,
            generation,
            entries: specs.iter().map(|spec| Entry { spec, open: false }).collect(),
            show: show::<R>,
            exists: |world| world.contains_resource::<R>(),
        });
        Ok(MenuHandle { provider: id, generation })
    }

    /// Removes a registration if `handle` is its current one.
    pub fn remove(&mut self, handle: MenuHandle) -> bool {
        let before = self.providers.len();
        self.providers.retain(|p| !(p.id == handle.provider && p.generation == handle.generation));
        self.providers.len() != before
    }

    /// Whether an entry's window is open.
    pub fn is_open(&self, handle: MenuHandle, id: &str) -> bool {
        self.entry(handle, id).is_some_and(|e| e.open)
    }

    /// Opens or closes an entry's window; `false` if there's no such entry.
    pub fn set_open(&mut self, handle: MenuHandle, id: &str, open: bool) -> bool {
        self.entry_mut(handle, id).map(|e| e.open = open).is_some()
    }

    /// The menu bar's dropdowns: categories by name, entries by name (then
    /// provider and ID, for a stable order).
    pub fn categories(&self) -> Vec<Category> {
        let mut categories: BTreeMap<&'static str, Vec<(&'static str, &'static str, MenuHandle, &'static str)>> = BTreeMap::new();
        for p in &self.providers {
            let handle = MenuHandle { provider: p.id, generation: p.generation };
            for e in &p.entries {
                categories.entry(e.spec.category).or_default().push((e.spec.name, p.name, handle, e.spec.id));
            }
        }
        categories
            .into_iter()
            .map(|(name, mut entries)| {
                entries.sort_by_key(|&(name, provider, _, id)| (name, provider, id));
                Category { name, entries: entries.into_iter().map(|(name, _, handle, id)| (name, handle, id)).collect() }
            })
            .collect()
    }

    fn entry(&self, handle: MenuHandle, id: &str) -> Option<&Entry> {
        let p = self.providers.iter().find(|p| p.id == handle.provider && p.generation == handle.generation)?;
        p.entries.iter().find(|e| e.spec.id == id)
    }

    fn entry_mut(&mut self, handle: MenuHandle, id: &str) -> Option<&mut Entry> {
        let p = self.providers.iter_mut().find(|p| p.id == handle.provider && p.generation == handle.generation)?;
        p.entries.iter_mut().find(|e| e.spec.id == id)
    }

    /// The open entries, in menu order, with what dispatch needs.
    fn open_entries(&self) -> Vec<(MenuHandle, MenuId, ShowFn)> {
        let mut open = Vec::new();
        for category in self.categories() {
            for (_, handle, id) in category.entries {
                let p = self.providers.iter().find(|p| p.id == handle.provider && p.generation == handle.generation).unwrap();
                let entry = p.entries.iter().find(|e| e.spec.id == id).unwrap();
                if entry.open {
                    open.push((handle, MenuId { provider: p.name, spec: entry.spec }, p.show));
                }
            }
        }
        open
    }
}

fn show<R: ImguiMenu>(world: &mut World, id: MenuId, ui: &mut egui::Ui, open: &mut bool) -> bool {
    if !world.contains_resource::<R>() {
        return false;
    }
    world.resource_scope(|world, mut provider: Mut<R>| provider.show_menu(id, ui, open, world));
    true
}

/// Drops registrations whose provider is no longer in the world, except
/// `keep` (a provider registering itself from its `init`).
fn prune(world: &mut World, keep: Option<TypeId>) {
    let Some(manager) = world.get_resource::<ImguiManager>() else { return };
    let stale: Vec<_> = manager
        .providers
        .iter()
        .filter(|p| Some(p.id) != keep && !(p.exists)(world))
        .map(|p| (MenuHandle { provider: p.id, generation: p.generation }, p.name))
        .collect();
    let mut manager = world.resource_mut::<ImguiManager>();
    for (handle, name) in stale {
        warn!("debug menu provider {name} left without unregistering; dropping its entries");
        manager.remove(handle);
    }
}

/// UI actions waiting to be applied once the pass has drawn: push a closure
/// from `show_menu` instead of changing the world there.
#[derive(Resource, Default)]
pub struct UiOutbox {
    actions: Vec<Box<dyn FnOnce(&mut World) + Send + Sync>>,
}

impl UiOutbox {
    pub fn push(&mut self, action: impl FnOnce(&mut World) + Send + Sync + 'static) {
        self.actions.push(Box::new(action));
    }

    pub fn len(&self) -> usize {
        self.actions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.actions.is_empty()
    }

    /// Drops what an earlier run of the same pass queued: only the last
    /// run's actions apply.
    pub(crate) fn clear(&mut self) {
        self.actions.clear();
    }
}

/// Applies the outbox, once per frame, after the pass.
pub(crate) fn apply_outbox(world: &mut World) {
    let actions = world.get_resource_mut::<UiOutbox>().map(|mut outbox| std::mem::take(&mut outbox.actions)).unwrap_or_default();
    for action in actions {
        action(world);
    }
}

/// One UI pass's content: the menu bar, then each open entry's window.
/// Runs inside the pass; may run more than once per frame.
pub(crate) fn draw(world: &mut World, ui: &mut egui::Ui) {
    if let Some(mut outbox) = world.get_resource_mut::<UiOutbox>() {
        outbox.clear();
    }
    prune(world, None);
    let open = {
        let Some(mut manager) = world.get_resource_mut::<ImguiManager>() else { return };
        egui::Panel::top(egui::Id::new("debug_ui_menu_bar")).show(ui, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                for category in manager.categories() {
                    ui.menu_button(category.name, |ui| {
                        for (name, handle, id) in category.entries {
                            let entry = manager.entry_mut(handle, id).unwrap();
                            ui.checkbox(&mut entry.open, name);
                        }
                    });
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.weak("/ hides the debug UI");
                });
            });
        });
        manager.open_entries()
    };
    // The manager is not borrowed while providers draw: they may register
    // or unregister. Each result is written back by handle afterwards.
    for (handle, id, show) in open {
        let mut still_open = true;
        let present = show(world, id, ui, &mut still_open);
        let Some(mut manager) = world.get_resource_mut::<ImguiManager>() else { return };
        if !present {
            warn!("debug menu provider of {} is gone; dropping its entries", id.name());
            manager.remove(handle);
        } else if !still_open {
            manager.set_open(handle, id.id(), false);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Resource, Default)]
    struct Tools {
        drawn: Vec<&'static str>,
        close: Option<&'static str>,
    }

    impl ImguiMenu for Tools {
        fn menus() -> &'static [MenuSpec] {
            const MENUS: &[MenuSpec] = &[MenuSpec::new("b", "Tools", "Zebra"), MenuSpec::new("a", "Tools", "Alpha"), MenuSpec::new("c", "Assets", "Cache")];
            MENUS
        }

        fn show_menu(&mut self, id: MenuId, root: &mut egui::Ui, open: &mut bool, world: &mut World) {
            assert!(!world.contains_resource::<Tools>(), "the provider is not in the world while it draws");
            self.drawn.push(id.id());
            if self.close == Some(id.id()) {
                *open = false;
            }
            id.window().open(open).show(root.ctx(), |ui| ui.label("body"));
        }
    }

    #[derive(Resource)]
    struct Clash;

    impl ImguiMenu for Clash {
        fn menus() -> &'static [MenuSpec] {
            const MENUS: &[MenuSpec] = &[MenuSpec::new("x", "Tools", "Alpha")];
            MENUS
        }
        fn show_menu(&mut self, _: MenuId, _: &mut egui::Ui, _: &mut bool, _: &mut World) {}
    }

    #[derive(Resource)]
    struct Twice;

    impl ImguiMenu for Twice {
        fn menus() -> &'static [MenuSpec] {
            const MENUS: &[MenuSpec] = &[MenuSpec::new("x", "A", "One"), MenuSpec::new("x", "A", "Two")];
            MENUS
        }
        fn show_menu(&mut self, _: MenuId, _: &mut egui::Ui, _: &mut bool, _: &mut World) {}
    }

    #[derive(Resource)]
    struct Unnamed;

    impl ImguiMenu for Unnamed {
        fn menus() -> &'static [MenuSpec] {
            const MENUS: &[MenuSpec] = &[MenuSpec::new("x", "A", " ")];
            MENUS
        }
        fn show_menu(&mut self, _: MenuId, _: &mut egui::Ui, _: &mut bool, _: &mut World) {}
    }

    fn world() -> World {
        let mut world = World::new();
        world.insert_resource(ImguiManager::default());
        world.insert_resource(UiOutbox::default());
        world
    }

    /// One pass over the world with an empty input.
    fn pass(ctx: &egui::Context, world: &mut World) {
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| draw(world, ui));
    }

    #[test]
    fn categories_and_entries_are_sorted_and_stable() {
        let mut world = world();
        world.insert_resource(Tools::default());
        let handle = ImguiManager::register::<Tools>(&mut world).unwrap();
        let categories = world.resource::<ImguiManager>().categories();
        assert_eq!(categories.iter().map(|c| c.name).collect::<Vec<_>>(), ["Assets", "Tools"]);
        assert_eq!(categories[1].entries, vec![("Alpha", handle, "a"), ("Zebra", handle, "b")]);
    }

    #[test]
    fn duplicates_and_empty_labels_are_rejected() {
        let mut world = world();
        world.insert_resource(Tools::default());
        ImguiManager::register::<Tools>(&mut world).unwrap();
        assert_eq!(ImguiManager::register::<Tools>(&mut world), Err(RegisterError::DuplicateProvider(type_name::<Tools>())));
        assert_eq!(ImguiManager::register::<Clash>(&mut world), Err(RegisterError::DuplicateName { category: "Tools", name: "Alpha" }));
        assert!(matches!(ImguiManager::register::<Twice>(&mut world), Err(RegisterError::DuplicateEntry { id: "x", .. })));
        assert!(matches!(ImguiManager::register::<Unnamed>(&mut world), Err(RegisterError::EmptyLabel { .. })));
        assert_eq!(ImguiManager::register::<Tools>(&mut World::new()), Err(RegisterError::NoManager));
    }

    #[test]
    fn only_open_entries_draw_including_collapsed_and_close_independently() {
        let mut world = world();
        world.insert_resource(Tools::default());
        let handle = ImguiManager::register::<Tools>(&mut world).unwrap();
        let ctx = egui::Context::default();
        pass(&ctx, &mut world);
        assert!(world.resource::<Tools>().drawn.is_empty(), "closed entries are not called");
        {
            let mut manager = world.resource_mut::<ImguiManager>();
            manager.set_open(handle, "a", true);
            manager.set_open(handle, "c", true);
        }
        // Collapse one window: egui then skips its body, but the provider
        // still runs.
        let collapsed = MenuId { provider: type_name::<Tools>(), spec: &Tools::menus()[1] };
        let mut state = egui::collapsing_header::CollapsingState::load_with_default_open(&ctx, collapsed.egui_id().with("collapsing"), true);
        state.set_open(false);
        state.store(&ctx);
        pass(&ctx, &mut world);
        assert_eq!(world.resource::<Tools>().drawn, ["c", "a"], "menu order: Assets before Tools");
        world.resource_mut::<Tools>().close = Some("a");
        world.resource_mut::<Tools>().drawn.clear();
        pass(&ctx, &mut world);
        let manager = world.resource::<ImguiManager>();
        assert!(!manager.is_open(handle, "a") && manager.is_open(handle, "c"));
        world.resource_mut::<Tools>().drawn.clear();
        pass(&ctx, &mut world);
        assert_eq!(world.resource::<Tools>().drawn, ["c"]);
    }

    #[test]
    fn unregistering_forgets_open_windows_and_stale_handles_remove_nothing() {
        let mut world = world();
        world.insert_resource(Tools::default());
        let first = ImguiManager::register::<Tools>(&mut world).unwrap();
        world.resource_mut::<ImguiManager>().set_open(first, "a", true);
        assert!(ImguiManager::unregister(&mut world, first));
        assert!(world.resource::<ImguiManager>().categories().is_empty(), "empty categories go");
        let second = ImguiManager::register::<Tools>(&mut world).unwrap();
        assert!(!world.resource::<ImguiManager>().is_open(second, "a"), "open state is discarded");
        assert!(!ImguiManager::unregister(&mut world, first), "a stale handle can't remove the newer registration");
        assert_eq!(world.resource::<ImguiManager>().categories().len(), 2);
    }

    #[test]
    fn a_provider_that_left_is_never_drawn_and_is_cleaned_up() {
        let mut world = world();
        world.insert_resource(Tools::default());
        let handle = ImguiManager::register::<Tools>(&mut world).unwrap();
        world.resource_mut::<ImguiManager>().set_open(handle, "a", true);
        world.remove_resource::<Tools>();
        pass(&egui::Context::default(), &mut world);
        assert!(world.resource::<ImguiManager>().categories().is_empty());
        // A later owner may register again.
        world.insert_resource(Tools::default());
        assert!(ImguiManager::register::<Tools>(&mut world).is_ok());
    }

    #[test]
    fn a_service_can_register_before_it_is_inserted() {
        let mut world = world();
        let handle = ImguiManager::register::<Tools>(&mut world).unwrap();
        world.insert_resource(Tools::default());
        world.resource_mut::<ImguiManager>().set_open(handle, "a", true);
        pass(&egui::Context::default(), &mut world);
        assert_eq!(world.resource::<Tools>().drawn, ["a"]);
    }

    #[derive(Resource, Default)]
    struct Counter(u32);

    #[derive(Resource)]
    struct Button;

    impl ImguiMenu for Button {
        fn menus() -> &'static [MenuSpec] {
            const MENUS: &[MenuSpec] = &[MenuSpec::new("b", "Actions", "Bump")];
            MENUS
        }
        fn show_menu(&mut self, _: MenuId, _: &mut egui::Ui, _: &mut bool, world: &mut World) {
            world.resource_mut::<UiOutbox>().push(|world| world.resource_mut::<Counter>().0 += 1);
        }
    }

    #[test]
    fn outbox_actions_apply_once_even_if_the_pass_reruns() {
        let mut world = world();
        world.init_resource::<Counter>();
        world.insert_resource(Button);
        let handle = ImguiManager::register::<Button>(&mut world).unwrap();
        world.resource_mut::<ImguiManager>().set_open(handle, "b", true);
        let ctx = egui::Context::default();
        ctx.memory_mut(|m| m.options.max_passes = std::num::NonZeroUsize::new(3).unwrap());
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            draw(&mut world, ui);
            ui.ctx().request_discard("test a second pass");
        });
        assert_eq!(world.resource::<UiOutbox>().len(), 1);
        apply_outbox(&mut world);
        apply_outbox(&mut world);
        assert_eq!(world.resource::<Counter>().0, 1);
    }
}
