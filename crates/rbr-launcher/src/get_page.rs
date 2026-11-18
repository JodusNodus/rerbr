//! Import (the sidebar page) with today's Get more body: source cards (gvrc.de's cars, the stage sites,
//! co-drivers), the recent downloads ReRBR recognises, and Add from the computer; and the Get more modal opened from
//! Cars, Stages and Co-drivers, which for now shows the matching part of it.

use egui::{Color32, RichText};

use crate::championship_page::common::{card, card_title};
use crate::kit;
use crate::pages::Page;
use crate::Form;

/// RallySimFans' download of the full set of stages.
const RSF_URL: &str = "https://www.rallysimfans.hu/rbr/download.php?download=rsfrbr";
const BTB_URL: &str = "http://btbfin.blogspot.com/";
/// RBR Czech's stage downloads, in English.
const RBR_CZ_URL: &str = "https://rbr.onlineracing.cz/index.php?act=download&cat=19&setlng=eng";
const VOICES_URL: &str = "https://luppisrbr.blogspot.com/p/compatible-co-driver-mods-v3.html";
const NOTES_URL: &str = "https://luppisrbr.blogspot.com/p/how-to-install-pacenotes.html";

/// A stage site's card: its title, what it is, and where it opens.
struct Site {
    title: &'static str,
    what: &'static str,
    url: &'static str,
}

const SITES: [Site; 3] = [
    Site {
        title: "Stages · RallySimFans",
        what: "The full set of stages. Install it, then choose its folder as your game folder.",
        url: RSF_URL,
    },
    Site {
        title: "BTB stages · BTBfin",
        what: "BTB stages from the community. Download one, then add it below.",
        url: BTB_URL,
    },
    Site {
        title: "Stages · RBR Czech",
        what: "Stage installers; download one, then add the .exe below (it isn't run). Most stages from before \
               mid-2023 work; newer ones are encrypted for RBR Czech's plugin and can't be used.",
        url: RBR_CZ_URL,
    },
];

const CODRIVERS_WHAT: &str =
    "Voices and pace notes. The lists are Luppis's; the voices are their authors'. V3 notes go \
                              with V3 co-drivers.";

/// A site card's Open website button.
fn open_site(ui: &mut egui::Ui, site: &Site) {
    if ui.button("Open website").clicked() {
        crate::crash::open(site.url.as_ref());
    }
}

/// The links to Luppis's lists of voices and of pace notes.
fn codriver_links(ui: &mut egui::Ui) {
    for (label, url) in [("Get co-drivers…", VOICES_URL), ("Get pace notes…", NOTES_URL)] {
        if ui.link(label).clicked() {
            crate::crash::open(url.as_ref());
        }
    }
}

/// A card the columns' width, with its title and its text in grey.
fn source_card(ui: &mut egui::Ui, width: f32, title: &str, what: &str, buttons: impl FnOnce(&mut egui::Ui)) {
    ui.allocate_ui(egui::vec2(width, 0.0), |ui| {
        card(ui, |ui| {
            ui.set_min_height(120.0);
            card_title(ui, title);
            ui.label(RichText::new(what).color(Color32::GRAY));
            ui.add_space(6.0);
            ui.horizontal_wrapped(buttons);
        });
    });
}

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

    /// The Get more modal, when open: today's cars list, stage sites or co-driver links.
    pub fn get_more_modal(&mut self, ctx: &egui::Context) {
        let Some(what) = self.get_more else { return };
        let mut close = false;
        let result = kit::modal(
            ctx,
            "get more",
            what.title(),
            None,
            |ui| match what {
                GetMore::Cars => {
                    // The list is the Cars modal's body: `download_cars_panel` draws it while `open`.
                    self.open_car_list();
                    if self.has_adding_output() {
                        ui.horizontal_wrapped(|ui| self.adding_job_ui(ui));
                        self.adding_status_ui(ui);
                    }
                    self.download_cars_panel(ui);
                }
                GetMore::Stages => {
                    let width = ui.available_width();
                    for site in &SITES {
                        source_card(ui, width, site.title, site.what, |ui| open_site(ui, site));
                    }
                }
                GetMore::Codrivers => {
                    source_card(ui, ui.available_width(), "Co-drivers & pace notes", CODRIVERS_WHAT, codriver_links)
                }
            },
            |ui| close = ui.button("Close").clicked(),
        );
        if close || result.closed() {
            self.get_more = None;
            if what == GetMore::Cars {
                self.close_car_list();
            }
        }
    }

    /// The Import page: a header, and for now today's Get more body.
    pub fn import_page(&mut self, ui: &mut egui::Ui) {
        kit::page_header(ui, "Import", Some("Add content from your downloads or your computer"), |_| {});
        kit::body(ui, |ui| self.get_page(ui));
    }

    /// Where new content comes from: the source cards, the recent downloads, and Add.
    pub(crate) fn get_page(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical().id_salt("get").show(ui, |ui| {
            ui.add_space(6.0);
            let gap = ui.spacing().item_spacing.x;
            let width = ((ui.available_width() - 2.0 * gap) / 3.0).floor();
            let site = |ui: &mut egui::Ui, site: &Site| {
                source_card(ui, width, site.title, site.what, |ui| open_site(ui, site));
            };
            ui.horizontal_top(|ui| {
                self.cars_card(ui, width);
                site(ui, &SITES[0]);
                site(ui, &SITES[1]);
            });
            ui.horizontal_top(|ui| {
                site(ui, &SITES[2]);
                self.codrivers_card(ui, width);
            });
            if self.has_adding_output() {
                ui.add_space(6.0);
                ui.horizontal_wrapped(|ui| self.adding_job_ui(ui));
                self.adding_status_ui(ui);
            }
            self.download_cars_panel(ui);
            self.ready_to_add_ui(ui);
            self.add_from_computer_ui(ui);
        });
    }

    /// The gvrc.de card: how many cars it has and how many are added; its button opens the list below the cards.
    fn cars_card(&mut self, ui: &mut egui::Ui, width: f32) {
        let added = self.adding.cars.len();
        let what = match self.downloads.available() {
            Some(n) => format!("NGP car packs · {n} available · {added} added"),
            None => format!("NGP car packs · {added} added"),
        };
        source_card(ui, width, "Cars · gvrc.de", &what, |ui| self.download_cars_button(ui));
    }

    /// The co-drivers card: Luppis's lists of voices and of pace notes.
    fn codrivers_card(&mut self, ui: &mut egui::Ui, width: f32) {
        source_card(ui, width, "Co-drivers & pace notes", CODRIVERS_WHAT, codriver_links);
    }
}
