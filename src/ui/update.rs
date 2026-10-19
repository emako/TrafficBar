//! "检查更新" against this program's Velopack release feed on GitHub or Gitee.
//! URLs can be overridden in config.ini (`[app] update_url_github/gitee`).

use std::sync::atomic::{AtomicI16, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;

use gpui::{App, AppContext, Context, InteractiveElement, IntoElement, ParentElement, Render, Styled, Window, div, px, size};
use gpui_component::{scroll::ScrollableElement, v_flex};
use velopack::sources::{GithubSource, UpdateSource};
use velopack::{UpdateCheck, UpdateInfo, UpdateManager};

use super::common::{button, footer, primary_button};
use super::Slot;
use crate::i18n::tr;
use crate::state;
use crate::update_source::GiteeSource;

const DEFAULT_GITHUB: &str = "https://github.com/emako/TrafficBar";
const DEFAULT_GITEE: &str = "https://gitee.com/emako/TrafficBar";

enum Outcome {
    Failed,
    NotInstalled,
    UpToDate,
    Available {
        version: String,
        notes: String,
        manager: UpdateManager,
        update: UpdateInfo,
    },
    Downloading {
        version: String,
        progress: i16,
    },
    DownloadFailed,
}

fn check_once(source_kind: i32, url: String) -> Outcome {
    let source: Box<dyn UpdateSource> = if source_kind == 1 {
        match GiteeSource::new(&url) {
            Ok(s) => Box::new(s),
            Err(_) => return Outcome::Failed,
        }
    } else {
        Box::new(GithubSource::new(&url, None, false))
    };
    let Ok(manager) = UpdateManager::new_boxed(source, None, None) else {
        return Outcome::NotInstalled;
    };
    match manager.check_for_updates() {
        Ok(UpdateCheck::UpdateAvailable(update)) => {
            let version = update.TargetFullRelease.Version.clone();
            let notes = update.TargetFullRelease.NotesMarkdown.clone();
            Outcome::Available {
                version,
                notes,
                manager,
                update: *update,
            }
        }
        Ok(UpdateCheck::NoUpdateAvailable) | Ok(UpdateCheck::RemoteIsEmpty) => Outcome::UpToDate,
        Err(_) => Outcome::Failed,
    }
}

pub fn check(silent: bool, cx: &mut App) {
    let already = state::with_mut(|s| std::mem::replace(&mut s.checking_update, true));
    if already {
        return;
    }
    // Read config on the UI thread; `state` is thread-local and empty on worker threads.
    let (source_kind, url) = state::with(|s| {
        let ini = crate::config::ini::Ini::load(&s.settings.config_path());
        if s.settings.general.update_source == 1 {
            (1, ini.get_string("app", "update_url_gitee", DEFAULT_GITEE))
        } else {
            (0, ini.get_string("app", "update_url_github", DEFAULT_GITHUB))
        }
    });
    cx.spawn(async move |cx| {
        let outcome = cx.background_spawn(async move { check_once(source_kind, url) }).await;
        let _ = cx.update(|cx| {
            state::with_mut(|s| s.checking_update = false);
            if silent && !matches!(outcome, Outcome::Available { .. }) {
                return;
            }
            super::close(Slot::Update, cx);
            super::open_window(Slot::Update, tr("IDS_VERSION_UPDATE"), size(px(480.), px(320.)), false, cx, move |_, _| UpdateView {
                outcome,
            });
        });
    })
    .detach();
}

pub struct UpdateView {
    outcome: Outcome,
}

impl UpdateView {
    fn start_download(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Outcome::Available {
            version,
            manager,
            update,
            ..
        } = std::mem::replace(
            &mut self.outcome,
            Outcome::Downloading {
                version: String::new(),
                progress: 0,
            },
        )
        else {
            return;
        };
        self.outcome = Outcome::Downloading {
            version: version.clone(),
            progress: 0,
        };
        cx.notify();

        let progress = Arc::new(AtomicI16::new(0));
        let progress_ui = progress.clone();
        let (done_tx, done_rx) = mpsc::channel();
        std::thread::spawn(move || {
            let (tx, rx) = mpsc::channel();
            let progress_thread = progress.clone();
            std::thread::spawn(move || {
                while let Ok(p) = rx.recv() {
                    progress_thread.store(p, Ordering::Relaxed);
                }
            });
            let result = manager.download_updates(&update, Some(tx));
            let _ = done_tx.send((result, manager, update));
        });

        cx.spawn(async move |this, cx| {
            loop {
                let p = progress_ui.load(Ordering::Relaxed);
                let _ = this.update(cx, |view, cx| {
                    if let Outcome::Downloading { progress, .. } = &mut view.outcome {
                        *progress = p;
                        cx.notify();
                    }
                });
                match done_rx.try_recv() {
                    Ok((Ok(()), manager, update)) => {
                        if let Err(err) = crate::app::apply_update_and_restart(&manager, &update) {
                            log::error!("apply update failed: {err}");
                            let _ = this.update(cx, |view, cx| {
                                view.outcome = Outcome::DownloadFailed;
                                cx.notify();
                            });
                        }
                        return;
                    }
                    Ok((Err(err), _, _)) => {
                        log::error!("download update failed: {err}");
                        let _ = this.update(cx, |view, cx| {
                            view.outcome = Outcome::DownloadFailed;
                            cx.notify();
                        });
                        return;
                    }
                    Err(mpsc::TryRecvError::Empty) => {}
                    Err(mpsc::TryRecvError::Disconnected) => {
                        let _ = this.update(cx, |view, cx| {
                            view.outcome = Outcome::DownloadFailed;
                            cx.notify();
                        });
                        return;
                    }
                }
                cx.background_spawn(async { std::thread::sleep(Duration::from_millis(200)) }).await;
            }
        })
        .detach();
        let _ = window;
    }
}

impl Render for UpdateView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (text, can_install) = match &self.outcome {
            Outcome::Failed => (tr("IDS_CHECK_UPDATE_FAILD"), false),
            Outcome::NotInstalled => (tr("BAR_UPDATE_NOT_INSTALLED"), false),
            Outcome::UpToDate => (
                format!(
                    "{}\n{}{}",
                    tr("IDS_ALREADY_UPDATED"),
                    tr("BAR_CURRENT_VERSION"),
                    env!("CARGO_PKG_VERSION")
                ),
                false,
            ),
            Outcome::Available { version, notes, .. } => {
                let text = if notes.trim().is_empty() {
                    tr("IDS_UPDATE_AVLIABLE").replace("%s", version)
                } else {
                    tr("IDS_UPDATE_AVLIABLE2")
                        .replacen("%s", version, 1)
                        .replacen("%s", notes.trim(), 1)
                };
                (text.replace("\r\n", "\n"), true)
            }
            Outcome::Downloading { version, progress } => (
                tr("BAR_UPDATE_DOWNLOADING")
                    .replacen("%s", version, 1)
                    .replacen("%s", &progress.to_string(), 1),
                false,
            ),
            Outcome::DownloadFailed => (tr("BAR_UPDATE_DOWNLOAD_FAILED"), false),
        };
        let mut buttons = Vec::new();
        if can_install {
            let entity = cx.entity().downgrade();
            buttons.push(primary_button("update-go", tr("BAR_DOWNLOAD"), move |w, cx| {
                let _ = entity.update(cx, |view, cx| view.start_download(w, cx));
            }));
        }
        let downloading = matches!(self.outcome, Outcome::Downloading { .. });
        buttons.push(button("update-close", tr("TXT_CLOSE"), downloading, |w, _| w.remove_window()));
        v_flex()
            .size_full()
            .child(
                v_flex()
                    .id("update-text")
                    .flex_1()
                    .min_h_0()
                    .p_4()
                    .overflow_y_scrollbar()
                    .children(text.split('\n').map(|l| div().text_sm().child(l.to_string()))),
            )
            .child(footer(buttons, cx))
    }
}
