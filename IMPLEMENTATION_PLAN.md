## Stage 1: Dual Hotkey + Recording Mode Flag
**Goal**: Register Ctrl+Shift+R alongside Ctrl+Shift+A, route events to correct handlers
**Success Criteria**: Both hotkeys registered, IDs exposed, app logs both registrations
**Status**: Complete

## Stage 2: Video Encoder (src/encoder.rs)
**Goal**: AVAssetWriter-based H.264/MP4 encoder using CVPixelBuffer
**Success Criteria**: VideoEncoder::new/start/append_frame/finish compile and link
**Status**: Complete

## Stage 3: Recording Coordinator (src/recording.rs)
**Goal**: RecordingState struct that captures frames and crops to selection
**Success Criteria**: capture_frame() captures full screen and crops via CGImage
**Status**: Complete

## Stage 4: Status Bar Stop Button
**Goal**: Toggle between camera icon + Capture menu and red dot + Stop Recording menu
**Success Criteria**: enter_recording_mode/exit_recording_mode switch UI correctly
**Status**: Complete

## Stage 5: Integration in AppDelegate
**Goal**: Wire recording flow: select region -> confirm -> start timer -> stop -> save
**Success Criteria**: Full recording lifecycle works end-to-end
**Status**: Complete

## Stage 6: Final Wiring
**Goal**: Add dependencies, module declarations, update Info.plist
**Success Criteria**: cargo build succeeds
**Status**: Complete
