use std::path::PathBuf;

use crate::annotation::model::Annotation;

/// A group of annotations drawn during a single pause, scoped to a frame range.
pub struct AnnotationSession {
    pub annotations: Vec<Annotation>,
    /// Frame index when this pause began (annotations appear from this frame).
    pub start_frame: usize,
    /// Frame index when the next pause began (annotations disappear at this frame).
    /// None means annotations persist until the end of the video.
    pub end_frame: Option<usize>,
}

/// State for the post-recording video editor.
pub struct EditorState {
    pub video_path: PathBuf,
    pub total_frames: usize,
    pub fps: f64,
    pub current_frame: usize,
    pub is_playing: bool,
    pub sessions: Vec<AnnotationSession>,
    /// Index into sessions for the session being annotated (while paused).
    pub active_session: Option<usize>,
}

impl EditorState {
    pub fn new(video_path: PathBuf, total_frames: usize, fps: f64) -> Self {
        EditorState {
            video_path,
            total_frames,
            fps,
            current_frame: 0,
            is_playing: false,
            sessions: Vec::new(),
            active_session: None,
        }
    }

    /// Called when the user pauses playback. Creates a new annotation session
    /// and closes the previous one's frame range.
    pub fn pause_at(&mut self, frame: usize) {
        // Close the previous active session's end_frame
        if let Some(prev_idx) = self.active_session {
            if let Some(prev) = self.sessions.get_mut(prev_idx) {
                if prev.end_frame.is_none() {
                    prev.end_frame = Some(frame);
                }
            }
        }

        let new_session = AnnotationSession {
            annotations: Vec::new(),
            start_frame: frame,
            end_frame: None,
        };
        self.sessions.push(new_session);
        self.active_session = Some(self.sessions.len() - 1);
        self.is_playing = false;
    }

    /// Called when the user resumes playback.
    pub fn play(&mut self) {
        self.is_playing = true;
    }

    /// Add an annotation to the current active session.
    pub fn add_annotation(&mut self, annotation: Annotation) {
        if let Some(idx) = self.active_session {
            if let Some(session) = self.sessions.get_mut(idx) {
                session.annotations.push(annotation);
            }
        }
    }

    /// Remove the last annotation from the active session.
    pub fn undo_annotation(&mut self) {
        if let Some(idx) = self.active_session {
            if let Some(session) = self.sessions.get_mut(idx) {
                session.annotations.pop();
            }
        }
    }

    /// Collect all annotations visible at a given frame index.
    pub fn annotations_at_frame(&self, frame: usize) -> Vec<&Annotation> {
        let mut result = Vec::new();
        for session in &self.sessions {
            if frame >= session.start_frame {
                let in_range = match session.end_frame {
                    Some(end) => frame < end,
                    None => true,
                };
                if in_range {
                    for ann in &session.annotations {
                        result.push(ann);
                    }
                }
            }
        }
        result
    }
}
