use std::path::PathBuf;

use objc2_core_foundation::CGPoint;

use crate::annotation::model::Annotation;

/// A single annotation with its own lifespan (frame range).
#[derive(Clone)]
pub struct TimedAnnotation {
    pub annotation: Annotation,
    pub start_frame: usize,
    /// None means the annotation persists until the end of the video.
    pub end_frame: Option<usize>,
}

/// State for the post-recording video editor.
pub struct EditorState {
    pub video_path: PathBuf,
    pub total_frames: usize,
    pub fps: f64,
    pub current_frame: usize,
    pub is_playing: bool,
    pub annotations: Vec<TimedAnnotation>,
    /// Index into annotations for the annotation currently being edited.
    pub active_annotation: Option<usize>,
    /// Redo stack for undone annotations.
    pub redo_stack: Vec<TimedAnnotation>,
}

impl EditorState {
    pub fn new(video_path: PathBuf, total_frames: usize, fps: f64) -> Self {
        EditorState {
            video_path,
            total_frames,
            fps,
            current_frame: 0,
            is_playing: false,
            annotations: Vec::new(),
            active_annotation: None,
            redo_stack: Vec::new(),
        }
    }

    /// Called when the user resumes playback.
    pub fn play(&mut self) {
        self.is_playing = true;
    }

    /// Add an annotation at the given frame. Sets it as active and returns its index.
    /// Default end frame is 1 second after start (capped at total frames).
    pub fn add_annotation(&mut self, annotation: Annotation, frame: usize) -> usize {
        self.redo_stack.clear();
        let one_second = (self.fps.round() as usize).max(1);
        let end = (frame + one_second).min(self.total_frames);
        let timed = TimedAnnotation {
            annotation,
            start_frame: frame,
            end_frame: Some(end),
        };
        self.annotations.push(timed);
        let idx = self.annotations.len() - 1;
        self.active_annotation = Some(idx);
        idx
    }

    /// Remove the active annotation, or pop the last annotation if none is active.
    /// Pushes the removed annotation to the redo stack.
    pub fn undo_annotation(&mut self) {
        if let Some(idx) = self.active_annotation.take() {
            if idx < self.annotations.len() {
                let removed = self.annotations.remove(idx);
                self.redo_stack.push(removed);
            }
        } else if let Some(ann) = self.annotations.pop() {
            self.redo_stack.push(ann);
        }
    }

    /// Redo the last undone annotation. Returns true if an annotation was restored.
    pub fn redo_annotation(&mut self) -> bool {
        if let Some(ann) = self.redo_stack.pop() {
            self.annotations.push(ann);
            self.active_annotation = Some(self.annotations.len() - 1);
            true
        } else {
            false
        }
    }

    /// Delete a specific annotation by index. Clears active_annotation if it matches.
    pub fn delete_annotation(&mut self, idx: usize) {
        if idx >= self.annotations.len() {
            return;
        }
        self.annotations.remove(idx);
        // Adjust or clear active_annotation
        match self.active_annotation {
            Some(active) if active == idx => {
                self.active_annotation = None;
            }
            Some(active) if active > idx => {
                self.active_annotation = Some(active - 1);
            }
            _ => {}
        }
    }

    /// Collect all annotations visible at a given frame index, with their indices.
    pub fn annotations_at_frame(&self, frame: usize) -> Vec<(usize, &Annotation)> {
        self.annotations
            .iter()
            .enumerate()
            .filter(|(_, ta)| {
                frame >= ta.start_frame
                    && ta.end_frame.map_or(true, |end| frame < end)
            })
            .map(|(i, ta)| (i, &ta.annotation))
            .collect()
    }

    /// Confirm the active annotation's end frame. Clears active_annotation.
    pub fn confirm_active(&mut self, frame: usize) {
        if let Some(idx) = self.active_annotation.take() {
            if let Some(ta) = self.annotations.get_mut(idx) {
                ta.end_frame = Some(frame.max(ta.start_frame + 1));
            }
        }
    }

    /// Set an annotation's start frame with validation.
    pub fn set_annotation_start(&mut self, idx: usize, frame: usize) {
        if let Some(ta) = self.annotations.get_mut(idx) {
            let max_start = ta.end_frame.map_or(self.total_frames.saturating_sub(1), |end| end.saturating_sub(1));
            ta.start_frame = frame.min(max_start);
        }
    }

    /// Set an annotation's end frame with validation.
    pub fn set_annotation_end(&mut self, idx: usize, frame: usize) {
        if let Some(ta) = self.annotations.get_mut(idx) {
            let clamped = frame.max(ta.start_frame + 1).min(self.total_frames);
            ta.end_frame = Some(clamped);
        }
    }

    /// Select an annotation by index for editing.
    pub fn select_annotation(&mut self, idx: usize) {
        if idx < self.annotations.len() {
            self.active_annotation = Some(idx);
        }
    }

    /// Deselect the active annotation.
    pub fn deselect_annotation(&mut self) {
        self.active_annotation = None;
    }

    /// Clear all annotations, redo stack, and active annotation (used after crop bakes them in).
    pub fn clear_all(&mut self) {
        self.annotations.clear();
        self.redo_stack.clear();
        self.active_annotation = None;
    }

    /// Hit-test annotations at a point for the given frame. Returns the topmost match.
    pub fn hit_test_annotation(&self, point: CGPoint, frame: usize) -> Option<usize> {
        // Iterate in reverse so topmost (last drawn) is found first
        let visible = self.annotations_at_frame(frame);
        for (idx, ann) in visible.into_iter().rev() {
            if ann.hit_test(point) {
                return Some(idx);
            }
        }
        None
    }

    /// Returns true if there are any annotations.
    pub fn has_any_annotations(&self) -> bool {
        !self.annotations.is_empty()
    }

    /// Get the active annotation's range, if any.
    pub fn active_annotation_range(&self) -> Option<(usize, Option<usize>)> {
        self.active_annotation.and_then(|idx| {
            self.annotations.get(idx).map(|ta| (ta.start_frame, ta.end_frame))
        })
    }
}
