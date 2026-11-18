//! The Get more modals opened from Cars, Stages and Co-drivers (gvrc.de's car list, the stage sites, Luppis's lists),
//! and the Import page (the sidebar's): drop zone and the last import (`content::ui`).

use egui::{Align, Layout, RichText};

use crate::kit;
use crate::pages::Page;
use crate::theme::*;
use crate::Form;

/// RallySimFans' download of the full set of stages.
const RSF_URL: &str = "https://www.rallysimfans.hu/rbr/download.php?download=rsfrbr";
const BTB_URL: &str = "http://btbfin.blogspot.com/";
/// RBR Czech's stage downloads, in English.
const RBR_CZ_URL: &str = "https://rbr.onlineracing.cz/index.php?act=download&cat=19&setlng=eng";
const VOICES_URL: &str = "https://luppisrbr.blogspot.com/p/compatible-co-driver-mods-v3.html";
const NOTES_URL: &str = "https://luppisrbr.blogspot.com/p/how-to-install-pacenotes.html";

/// A site of a Get more modal: its title, where it is, what it is and what to do after downloading, and its URL.
struct Site {
    title: &'static str,
    host: &'static str,
    what: &'static str,
    url: &'static str,
}

const STAGE_SITES: [Site; 3] = [
    Site {
        title: "RallySimFans",
        host: "rallysimfans.hu",
        what: "The full set of stages. Install it, then choose its folder as your game folder in Settings.",
        url: RSF_URL,
    },
    Site {
        title: "BTBfin",
        host: "btbfin.blogspot.com",
        what: "BTB stages from the community. Download one, then add it on Import.",
        url: BTB_URL,
    },
    Site {
        title: "RBR Czech",
        host: "rbr.onlineracing.cz",
        what: "Stage installers; download one, then add the .exe on Import (it isn't run). Most stages from before \
               mid-2023 work; newer ones are encrypted for RBR Czech's plugin and can't be used.",
        url: RBR_CZ_URL,
    },
];

const CODRIVER_SITES: [Site; 2] = [
    Site {
        title: "Co-drivers",
        host: "luppisrbr.blogspot.com",
        what: "Luppis's list of voices, each with its author's download. Download one, then add it on Import.",
        url: VOICES_URL,
    },
    Site {
        title: "Pace notes",
        host: "luppisrbr.blogspot.com",
        what: "Luppis's guide to the pace notes. V3 notes go with V3 co-drivers; add them on Import too.",
        url: NOTES_URL,
    },
];

const STAGES_WHAT: &str = "Download from one of these sites, then add the files on Import. ReRBR works out what \
                           each one is.";
const CODRIVERS_WHAT: &str = "Voices and pace notes. The lists are Luppis's; the voices are their authors'.";

/// What the Get more modal offers: gvrc.de's cars, the stage sites or the co-driver links.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GetMore {
    Cars,
    Stages,
    Codrivers,
}

impl GetMore {
    fn title(self) -> &'static str {
        match self {
            GetMore::Cars => "Get more cars",
            GetMore::Stages => "Get more stages",
            GetMore::Codrivers => "Get more co-drivers",
        }
    }
}

/// The sites' rows: a title with its host, what it is, and Open site.
fn sites_ui(ui: &mut egui::Ui, sites: &[Site]) {
    kit::panel(ui, |rows| {
        for site in sites {
            rows.full(|ui| {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.button("Open site ↗").clicked() {
                        crate::crash::open(site.url.as_ref());
                    }
                    ui.with_layout(Layout::top_down(Align::Min), |ui| {
                        ui.spacing_mut().item_spacing.y = 3.0;
                        ui.add_space(10.0);
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(site.title).family(medium()).color(TEXT));
                            ui.label(RichText::new(site.host).font(mono(11.5)).color(TEXT_3));
                        });
                        ui.add(egui::Label::new(RichText::new(site.what).size(12.0).color(TEXT_2)).wrap());
                        ui.add_space(10.0);
                    });
                });
            });
        }
    });
}

impl Form {
    /// Opens the Get more modal over its page (Cars, Stages or Co-drivers).
    pub fn open_get_more(&mut self, what: GetMore) {
        self.page = match what {
            GetMore::Cars => Page::Cars,
            GetMore::Stages => Page::Stages,
            GetMore::Codrivers => Page::Codrivers,
        };
        self.get_more = Some(what);
        if what == GetMore::Cars {
            self.open_car_list();
        }
    }

    /// The Get more modal, when open.
    pub fn get_more_modal(&mut self, ctx: &egui::Context) {
        let Some(what) = self.get_more else { return };
        let (mut close, mut import) = (false, false);
        let result = match what {
            GetMore::Cars => {
                self.open_car_list();
                let subtitle = self.cars_subtitle();
                let foot = self.cars_footer();
                let mut action = None;
                let result = kit::modal(
                    ctx,
                    "get more",
                    what.title(),
                    Some(&subtitle),
                    |ui| self.cars_body(ui, &mut import),
                    |ui| action = crate::gvrc::cars_footer_ui(&foot, ui),
                );
                if let Some(action) = action {
                    self.apply_cars_action(action);
                }
                result
            }
            GetMore::Stages | GetMore::Codrivers => {
                let (subtitle, sites): (_, &[Site]) = match what {
                    GetMore::Stages => (STAGES_WHAT, &STAGE_SITES),
                    _ => (CODRIVERS_WHAT, &CODRIVER_SITES),
                };
                kit::modal(
                    ctx,
                    "get more",
                    what.title(),
                    Some(subtitle),
                    |ui| sites_ui(ui, sites),
                    |ui| {
                        close = ui.button("Done").clicked();
                        ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                            ui.label(RichText::new("Downloaded something?").color(TEXT_2));
                            import = kit::link(ui, "Import files…").clicked();
                        });
                    },
                )
            }
        };
        if import {
            self.page = Page::Import;
        }
        if close || import || result.closed() {
            self.get_more = None;
            if what == GetMore::Cars {
                self.close_car_list();
            }
        }
    }

    /// The Import page: the drop zone and the last import.
    pub fn import_page(&mut self, ui: &mut egui::Ui) {
        kit::page_header(
            ui,
            "Import",
            Some("ReRBR works out what each file is. Your downloads aren't changed."),
            |_| {},
        );
        kit::body(ui, |ui| {
            egui::ScrollArea::vertical().id_salt("import").auto_shrink([false, false]).show(ui, |ui| {
                ui.set_max_width(680.0);
                self.drop_zone_ui(ui);
                ui.add_space(20.0);
                self.last_import_ui(ui);
                ui.add_space(10.0);
                kit::hint(ui, "RAR archives and installers other than RBR Czech's can't be read: extract them first.");
            });
        });
    }
}
