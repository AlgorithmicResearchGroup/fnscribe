use block2::RcBlock;
use core_foundation::base::TCFType;
use core_foundation::boolean::CFBoolean;
use core_foundation::dictionary::{CFDictionary, CFDictionaryRef};
use core_foundation::runloop::CFRunLoop;
use core_foundation::string::{CFString, CFStringRef};
use core_graphics::event::{
    CGEventFlags, CGEventTap, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement,
    CGEventType, CallbackResult, EventField,
};
use enigo::{Enigo, Keyboard, Settings};
use objc2::runtime::Bool;
use objc2_app_kit::{NSApplicationActivationOptions, NSRunningApplication, NSWorkspace};
use objc2_av_foundation::{AVAuthorizationStatus, AVCaptureDevice, AVMediaType, AVMediaTypeAudio};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

const FUNCTION_KEY_CODE: i64 = 0x003f;

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXIsProcessTrustedWithOptions(options: CFDictionaryRef) -> bool;
    static kAXTrustedCheckOptionPrompt: CFStringRef;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FnEvent {
    Ready,
    Pressed,
    Released,
    Unavailable,
}

pub fn accessibility_trusted() -> bool {
    check_accessibility(false)
}

pub fn request_accessibility() {
    let _ = check_accessibility(true);
}

pub fn microphone_trusted() -> bool {
    microphone_permission() == AVAuthorizationStatus::Authorized
}

pub fn request_microphone() {
    let status = microphone_permission();
    if status == AVAuthorizationStatus::NotDetermined {
        prompt_for_microphone();
    } else if status != AVAuthorizationStatus::Authorized {
        let _ = Command::new("/usr/bin/open")
            .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone")
            .spawn();
    }
}

pub fn prompt_for_microphone_if_needed() {
    if microphone_permission() == AVAuthorizationStatus::NotDetermined {
        prompt_for_microphone();
    }
}

pub fn frontmost_application_pid() -> Option<i32> {
    NSWorkspace::sharedWorkspace()
        .frontmostApplication()
        .map(|application| application.processIdentifier())
}

pub fn activate_application(pid: i32) -> bool {
    NSRunningApplication::runningApplicationWithProcessIdentifier(pid).is_some_and(|application| {
        application.activateWithOptions(NSApplicationActivationOptions::empty())
    })
}

pub fn insert_text(text: &str) -> Result<(), String> {
    let settings = Settings {
        open_prompt_to_get_permissions: false,
        ..Settings::default()
    };
    let mut enigo = Enigo::new(&settings)
        .map_err(|_| "Accessibility permission is required to insert text.".to_string())?;
    enigo
        .text(text)
        .map_err(|error| format!("Could not insert the transcription: {error}"))
}

pub fn start_fn_monitor(callback: impl Fn(FnEvent) + Send + Sync + 'static) {
    let (event_sender, event_receiver) = mpsc::channel();
    thread::spawn(move || {
        for event in event_receiver {
            callback(event);
        }
    });

    thread::spawn(move || {
        loop {
            if !accessibility_trusted() {
                thread::sleep(Duration::from_millis(500));
                continue;
            }

            let fn_is_down = AtomicBool::new(false);
            let callback_sender = event_sender.clone();
            let ready_sender = event_sender.clone();
            let result = CGEventTap::with_enabled(
                CGEventTapLocation::HID,
                CGEventTapPlacement::HeadInsertEventTap,
                // An active pass-through tap is authorized by Accessibility.
                // A ListenOnly tap would require a second Input Monitoring grant.
                CGEventTapOptions::Default,
                vec![
                    CGEventType::FlagsChanged,
                    CGEventType::KeyDown,
                    CGEventType::KeyUp,
                ],
                move |_proxy, event_type, event| {
                    let event_type = event_type as u32;
                    let key_code =
                        event.get_integer_value_field(EventField::KEYBOARD_EVENT_KEYCODE);
                    let flags_show_fn = event
                        .get_flags()
                        .contains(CGEventFlags::CGEventFlagSecondaryFn);

                    let next_state = if key_code == FUNCTION_KEY_CODE
                        && event_type == CGEventType::KeyDown as u32
                    {
                        Some(true)
                    } else if key_code == FUNCTION_KEY_CODE
                        && event_type == CGEventType::KeyUp as u32
                    {
                        Some(false)
                    } else if event_type == CGEventType::FlagsChanged as u32
                        && (key_code == FUNCTION_KEY_CODE
                            || flags_show_fn != fn_is_down.load(Ordering::Relaxed))
                    {
                        Some(flags_show_fn)
                    } else {
                        None
                    };

                    if let Some(is_down) = next_state {
                        let was_down = fn_is_down.swap(is_down, Ordering::Relaxed);
                        if is_down != was_down {
                            let _ = callback_sender.send(if is_down {
                                FnEvent::Pressed
                            } else {
                                FnEvent::Released
                            });
                        }
                    }
                    CallbackResult::Keep
                },
                move || {
                    let _ = ready_sender.send(FnEvent::Ready);
                    CFRunLoop::run_current();
                },
            );

            if result.is_err() {
                let _ = event_sender.send(FnEvent::Unavailable);
                thread::sleep(Duration::from_secs(1));
            }
        }
    });
}

fn check_accessibility(show_prompt: bool) -> bool {
    let key = unsafe { CFString::wrap_under_get_rule(kAXTrustedCheckOptionPrompt) };
    let value = if show_prompt {
        CFBoolean::true_value()
    } else {
        CFBoolean::false_value()
    };
    let options = CFDictionary::from_CFType_pairs(&[(key, value)]);
    unsafe { AXIsProcessTrustedWithOptions(options.as_concrete_TypeRef()) }
}

fn microphone_permission() -> AVAuthorizationStatus {
    unsafe { AVCaptureDevice::authorizationStatusForMediaType(audio_media_type()) }
}

fn prompt_for_microphone() {
    let completion = RcBlock::new(|_granted: Bool| {});
    unsafe {
        AVCaptureDevice::requestAccessForMediaType_completionHandler(
            audio_media_type(),
            &completion,
        );
    }
}

fn audio_media_type() -> &'static AVMediaType {
    unsafe { AVMediaTypeAudio.expect("AVFoundation did not provide its audio media type") }
}
