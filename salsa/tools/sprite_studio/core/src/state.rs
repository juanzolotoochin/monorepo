//! Pure UI-state machine driven by the GUI layer.

use crate::project::{Action, Project, DEFAULT_FPS};
use std::path::PathBuf;

#[derive(Debug, Default)]
pub struct AppState {
    pub project: Option<Project>,
    pub project_dir: Option<PathBuf>,
    pub selected_action: Option<usize>,
    pub current_frame: usize,
    pub playing: bool,
}

impl AppState {
    /// Installs a freshly opened/imported project: selects the first action
    /// (if any), resets to frame 0, and starts playing when non-empty.
    pub fn set_project(&mut self, project: Project, dir: PathBuf) {
        let has_actions = !project.actions.is_empty();
        self.selected_action = has_actions.then_some(0);
        self.current_frame = 0;
        self.playing = has_actions;
        self.project = Some(project);
        self.project_dir = Some(dir);
    }

    pub fn selected(&self) -> Option<&Action> {
        let project = self.project.as_ref()?;
        project.actions.get(self.selected_action?)
    }

    pub fn select_action(&mut self, index: usize) {
        self.selected_action = Some(index);
        self.current_frame = 0;
    }

    pub fn toggle_play(&mut self) {
        self.playing = !self.playing;
    }

    /// Advances to the next frame, wrapping within the selected action.
    pub fn advance_frame(&mut self) {
        let n = self.selected().map(|a| a.frames.len()).unwrap_or(0);
        if n > 0 {
            self.current_frame = (self.current_frame + 1) % n;
        }
    }

    /// Pauses and jumps to a specific frame.
    pub fn scrub_to(&mut self, frame: usize) {
        let n = self.selected().map(|a| a.frames.len()).unwrap_or(0);
        self.playing = false;
        self.current_frame = if n == 0 { 0 } else { frame.min(n - 1) };
    }

    pub fn current_fps(&self) -> u32 {
        self.selected().map(|a| a.fps).unwrap_or(DEFAULT_FPS)
    }

    /// Sets the selected action's FPS (in memory, clamped to >= 1).
    pub fn set_selected_fps(&mut self, fps: u32) {
        if let (Some(project), Some(i)) = (self.project.as_mut(), self.selected_action) {
            if let Some(action) = project.actions.get_mut(i) {
                action.fps = fps.max(1);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project_with(frames_per_action: &[usize]) -> Project {
        let actions = frames_per_action
            .iter()
            .enumerate()
            .map(|(i, &n)| Action {
                name: format!("A{i}"),
                fps: 10 + i as u32,
                frames: (0..n).map(|f| PathBuf::from(format!("sprites/A{i}/{f:03}.png"))).collect(),
            })
            .collect();
        Project { version: 1, name: "t".into(), actions }
    }

    #[test]
    fn set_project_selects_first_action_and_plays() {
        let mut s = AppState::default();
        s.set_project(project_with(&[3, 2]), PathBuf::from("/p"));
        assert_eq!(s.selected_action, Some(0));
        assert_eq!(s.current_frame, 0);
        assert!(s.playing);
        assert_eq!(s.current_fps(), 10);
    }

    #[test]
    fn set_empty_project_selects_nothing_and_pauses() {
        let mut s = AppState::default();
        s.set_project(project_with(&[]), PathBuf::from("/p"));
        assert_eq!(s.selected_action, None);
        assert!(!s.playing);
        assert_eq!(s.current_fps(), DEFAULT_FPS);
    }

    #[test]
    fn select_action_resets_frame_and_updates_fps() {
        let mut s = AppState::default();
        s.set_project(project_with(&[3, 2]), PathBuf::from("/p"));
        s.current_frame = 2;
        s.select_action(1);
        assert_eq!(s.selected_action, Some(1));
        assert_eq!(s.current_frame, 0);
        assert_eq!(s.current_fps(), 11);
    }

    #[test]
    fn advance_frame_wraps_within_action() {
        let mut s = AppState::default();
        s.set_project(project_with(&[3]), PathBuf::from("/p"));
        s.advance_frame();
        assert_eq!(s.current_frame, 1);
        s.advance_frame();
        s.advance_frame();
        assert_eq!(s.current_frame, 0); // wrapped 2 -> 0
    }

    #[test]
    fn scrub_pauses_and_sets_frame() {
        let mut s = AppState::default();
        s.set_project(project_with(&[3]), PathBuf::from("/p"));
        assert!(s.playing);
        s.scrub_to(2);
        assert!(!s.playing);
        assert_eq!(s.current_frame, 2);
    }

    #[test]
    fn scrub_clamps_to_last_frame_when_out_of_range() {
        let mut s = AppState::default();
        s.set_project(project_with(&[3]), PathBuf::from("/p"));
        s.scrub_to(99);
        assert_eq!(s.current_frame, 2); // clamped to len-1
    }

    #[test]
    fn toggle_play_flips_state() {
        let mut s = AppState::default();
        s.set_project(project_with(&[3]), PathBuf::from("/p"));
        s.toggle_play();
        assert!(!s.playing);
        s.toggle_play();
        assert!(s.playing);
    }

    #[test]
    fn set_selected_fps_updates_and_clamps() {
        let mut s = AppState::default();
        s.set_project(project_with(&[3]), PathBuf::from("/p"));
        s.set_selected_fps(24);
        assert_eq!(s.current_fps(), 24);
        s.set_selected_fps(0);
        assert_eq!(s.current_fps(), 1); // clamped to >= 1
    }
}
