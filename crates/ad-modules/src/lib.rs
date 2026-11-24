//! 1:1 Decompiled After Dark 1.0a (1991) Screensaver Modules.
//!
//! Phase 1 includes:
//! - **Flying Toasters** (`TOASTERS.AD`, exact 1:1 decompilation + original 12 BMPs)
//! - **Starry Night** (`AD.EXE SEG 2`, exact 1:1 decompilation)
//! - **Clocks** (`CLOCKS.AD`, exact 1:1 decompilation + original 14 BMPs + PC-speaker sound)
//! - **Nocturnes** (`NOCTURNE.AD`, exact 1:1 decompilation + original 102×80 EYES sprite sheet)
//! - **Randomizer** (`AD.EXE SEG 16`, exact 1:1 decompilation + `RANDOM.ADS` compatibility)
//! - **Blanker** (`ADMODULE.SDK\BLANKER`, modernized Win64 SDK sample)

pub mod aqua;
pub mod clocks;
pub mod draino;
pub mod fade;
pub mod geobounce;
pub mod globe;
pub mod grafstat;
pub mod grav;
pub mod laser;
pub mod logo;
pub mod looney;
pub mod magic;
pub mod message;
pub mod mondrian;
pub mod mountain;
pub mod nocturne;
pub mod punch;
pub mod puzzle;
pub mod rain;
pub mod rainstor;
pub mod randomizer;
pub mod rose;
pub mod satori;
pub mod sgyra;
pub mod shapes;
pub mod simpsons;
pub mod slidesho;
pub mod spheres;
pub mod spotlite;
pub mod stained;
pub mod starry;
pub mod string_theory;
pub mod tng;
pub mod toasters;
pub mod vertigo;
pub mod warp;
pub mod worms;
pub mod wrap;
pub mod zot;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use ad_blanker_sdk_example::BlankerModule;
use ad_sdk::ffi::DynamicModule;
use ad_sdk::{AdMetadata, AdModule};

use aqua::AquaticRealmModule;
use clocks::ClocksModule;
use draino::DrainoModule;
use fade::FadeAwayModule;
use geobounce::GeoBounceModule;
use globe::GlobeModule;
use grafstat::GraphStatModule;
use grav::GravityModule;
use laser::LaserModule;
use logo::LogoModule;
use looney::acme_shopping::AcmeShoppingModule;
use looney::cartoons_101::Cartoons101Module;
use looney::conducktor::ConducktorModule;
use looney::desquetoppe::DesquetoppeModule;
use looney::marvins_invasion::MarvinsInvasionModule;
use looney::messages::LooneyMessagesModule;
use looney::michigan_frog::MichiganFrogModule;
use looney::putty_tat::PuttyTatModule;
use looney::rabbitron::RabbitronModule;
use looney::taz_desktop::TazDesktopModule;
use looney::wockets::WocketsModule;
use looney::yosemite_sam::YosemiteSamModule;
use magic::MagicModule;
use message::MessagesModule;
use mondrian::MondrianModule;
use mountain::MountainsModule;
use nocturne::NocturneModule;
use punch::PunchOutModule;
use puzzle::PuzzleModule;
use rain::HardRainModule;
use rainstor::RainStormModule;
use randomizer::RandomizerModule;
use rose::RoseModule;
use satori::SatoriModule;
use sgyra::SpiralGyraModule;
use shapes::ShapesModule;
use simpsons::chalkboard::ChalkboardModule;
use simpsons::files::SimpsonsFilesModule;
use simpsons::grampa::GrampaModule;
use simpsons::grass_skirts::GrassSkirtsModule;
use simpsons::homer_eats::HomerEatsModule;
use simpsons::how_to_draw::HowToDrawModule;
use simpsons::itchy_scratchy::ItchyScratchyModule;
use simpsons::krusty::KrustyModule;
use simpsons::lisa_moods::LisaMoodsModule;
use simpsons::mr_burns::MrBurnsModule;
use simpsons::objets_bart::ObjetsBartModule;
use simpsons::physics::SimpsonsPhysicsModule;
use simpsons::simpsons_clocks::SimpsonsClocksModule;
use simpsons::snowball::SnowballModule;
use simpsons::trivia::SimpsonsTriviaModule;
use slidesho::SlideShowModule;
use spheres::SpheresModule;
use spotlite::SpotlightModule;
use stained::StainedGlassModule;
use starry::StarryNightModule;
use string_theory::StringTheoryModule;
use tng::borg::TheBorgModule;
use tng::data_dances::DataDancesModule;
use tng::encounters::EncountersModule;
use tng::nanites::NanitesModule;
use tng::officers_review::OfficersReviewModule;
use tng::personnel_files::PersonnelFilesModule;
use tng::science_stations::ScienceStationsModule;
use tng::starbase::StarbaseModule;
use tng::starfleet_messages::StarfleetMessagesModule;
use tng::tachyon::TachyonModule;
use tng::troi::CounselorTroiModule;
use tng::warp_effect::WarpEffectModule;
use tng::worfs_weapons::WorfsWeaponsModule;
use toasters::ToastersModule;
use vertigo::VertigoModule;
use warp::WarpModule;
use worms::WormsModule;
use wrap::WrapAroundModule;
use zot::ZotModule;

/// Descriptor for an available module (built-in decompiled Rust module or external `.so` plugin).
#[derive(Debug, Clone)]
pub struct ModuleCatalogEntry {
    pub meta: AdMetadata,
    pub external_dll: Option<PathBuf>,
}

/// Build the full sorted catalog of built-in modules plus any external SDK `.so` plugins
/// placed in `plugin_dir`.
pub fn build_catalog(plugin_dir: Option<&Path>) -> Vec<ModuleCatalogEntry> {
    let mut entries = vec![
        ModuleCatalogEntry { meta: AquaticRealmModule::static_metadata(), external_dll: None },
        ModuleCatalogEntry { meta: BlankerModule::new().metadata(), external_dll: None },
        ModuleCatalogEntry { meta: ClocksModule::static_metadata(), external_dll: None },
        ModuleCatalogEntry { meta: DrainoModule::static_metadata(), external_dll: None },
        ModuleCatalogEntry { meta: FadeAwayModule::static_metadata(), external_dll: None },
        ModuleCatalogEntry { meta: ToastersModule::static_metadata(), external_dll: None },
        ModuleCatalogEntry { meta: GeoBounceModule::static_metadata(), external_dll: None },
        ModuleCatalogEntry { meta: GlobeModule::static_metadata(), external_dll: None },
        ModuleCatalogEntry { meta: GraphStatModule::static_metadata(), external_dll: None },
        ModuleCatalogEntry { meta: GravityModule::static_metadata(), external_dll: None },
        ModuleCatalogEntry { meta: HardRainModule::static_metadata(), external_dll: None },
        ModuleCatalogEntry { meta: LaserModule::static_metadata(), external_dll: None },
        ModuleCatalogEntry { meta: LogoModule::static_metadata(), external_dll: None },
        ModuleCatalogEntry { meta: MagicModule::static_metadata(), external_dll: None },
        ModuleCatalogEntry { meta: MessagesModule::static_metadata(), external_dll: None },
        ModuleCatalogEntry { meta: MondrianModule::static_metadata(), external_dll: None },
        ModuleCatalogEntry { meta: MountainsModule::static_metadata(), external_dll: None },
        ModuleCatalogEntry { meta: NocturneModule::static_metadata(), external_dll: None },
        ModuleCatalogEntry { meta: PunchOutModule::static_metadata(), external_dll: None },
        ModuleCatalogEntry { meta: PuzzleModule::static_metadata(), external_dll: None },
        ModuleCatalogEntry { meta: RainStormModule::static_metadata(), external_dll: None },
        ModuleCatalogEntry { meta: RandomizerModule::static_metadata(), external_dll: None },
        ModuleCatalogEntry { meta: RoseModule::static_metadata(), external_dll: None },
        ModuleCatalogEntry { meta: SatoriModule::static_metadata(), external_dll: None },
        ModuleCatalogEntry { meta: ShapesModule::static_metadata(), external_dll: None },
        ModuleCatalogEntry { meta: SlideShowModule::static_metadata(), external_dll: None },
        ModuleCatalogEntry { meta: SpheresModule::static_metadata(), external_dll: None },
        ModuleCatalogEntry { meta: SpiralGyraModule::static_metadata(), external_dll: None },
        ModuleCatalogEntry { meta: SpotlightModule::static_metadata(), external_dll: None },
        ModuleCatalogEntry { meta: StainedGlassModule::static_metadata(), external_dll: None },
        ModuleCatalogEntry { meta: StarryNightModule::static_metadata(), external_dll: None },
        ModuleCatalogEntry { meta: StringTheoryModule::static_metadata(), external_dll: None },
        ModuleCatalogEntry { meta: VertigoModule::static_metadata(), external_dll: None },
        ModuleCatalogEntry { meta: WarpModule::static_metadata(), external_dll: None },
        ModuleCatalogEntry { meta: WormsModule::static_metadata(), external_dll: None },
        ModuleCatalogEntry { meta: WrapAroundModule::static_metadata(), external_dll: None },
        ModuleCatalogEntry { meta: ZotModule::static_metadata(), external_dll: None },
        // 1994 The Simpsons After Dark 3.0 Pack (15 modules)
        ModuleCatalogEntry { meta: ChalkboardModule::static_metadata(), external_dll: None },
        ModuleCatalogEntry { meta: SimpsonsFilesModule::static_metadata(), external_dll: None },
        ModuleCatalogEntry { meta: GrampaModule::static_metadata(), external_dll: None },
        ModuleCatalogEntry { meta: GrassSkirtsModule::static_metadata(), external_dll: None },
        ModuleCatalogEntry { meta: HomerEatsModule::static_metadata(), external_dll: None },
        ModuleCatalogEntry { meta: HowToDrawModule::static_metadata(), external_dll: None },
        ModuleCatalogEntry { meta: ItchyScratchyModule::static_metadata(), external_dll: None },
        ModuleCatalogEntry { meta: KrustyModule::static_metadata(), external_dll: None },
        ModuleCatalogEntry { meta: LisaMoodsModule::static_metadata(), external_dll: None },
        ModuleCatalogEntry { meta: MrBurnsModule::static_metadata(), external_dll: None },
        ModuleCatalogEntry { meta: ObjetsBartModule::static_metadata(), external_dll: None },
        ModuleCatalogEntry { meta: SimpsonsPhysicsModule::static_metadata(), external_dll: None },
        ModuleCatalogEntry { meta: SimpsonsClocksModule::static_metadata(), external_dll: None },
        ModuleCatalogEntry { meta: SimpsonsTriviaModule::static_metadata(), external_dll: None },
        ModuleCatalogEntry { meta: SnowballModule::static_metadata(), external_dll: None },
        // 1994 Star Trek: The Next Generation After Dark 3.0 Pack (13 modules)
        ModuleCatalogEntry { meta: CounselorTroiModule::static_metadata(), external_dll: None },
        ModuleCatalogEntry { meta: DataDancesModule::static_metadata(), external_dll: None },
        ModuleCatalogEntry { meta: EncountersModule::static_metadata(), external_dll: None },
        ModuleCatalogEntry { meta: NanitesModule::static_metadata(), external_dll: None },
        ModuleCatalogEntry { meta: OfficersReviewModule::static_metadata(), external_dll: None },
        ModuleCatalogEntry { meta: PersonnelFilesModule::static_metadata(), external_dll: None },
        ModuleCatalogEntry { meta: ScienceStationsModule::static_metadata(), external_dll: None },
        ModuleCatalogEntry { meta: StarbaseModule::static_metadata(), external_dll: None },
        ModuleCatalogEntry { meta: StarfleetMessagesModule::static_metadata(), external_dll: None },
        ModuleCatalogEntry { meta: TachyonModule::static_metadata(), external_dll: None },
        ModuleCatalogEntry { meta: TheBorgModule::static_metadata(), external_dll: None },
        ModuleCatalogEntry { meta: WarpEffectModule::static_metadata(), external_dll: None },
        ModuleCatalogEntry { meta: WorfsWeaponsModule::static_metadata(), external_dll: None },
        // 1995 Looney Tunes After Dark 3.0 Pack (12 modules)
        ModuleCatalogEntry { meta: AcmeShoppingModule::static_metadata(), external_dll: None },
        ModuleCatalogEntry { meta: Cartoons101Module::static_metadata(), external_dll: None },
        ModuleCatalogEntry { meta: ConducktorModule::static_metadata(), external_dll: None },
        ModuleCatalogEntry { meta: DesquetoppeModule::static_metadata(), external_dll: None },
        ModuleCatalogEntry { meta: MarvinsInvasionModule::static_metadata(), external_dll: None },
        ModuleCatalogEntry { meta: LooneyMessagesModule::static_metadata(), external_dll: None },
        ModuleCatalogEntry { meta: MichiganFrogModule::static_metadata(), external_dll: None },
        ModuleCatalogEntry { meta: PuttyTatModule::static_metadata(), external_dll: None },
        ModuleCatalogEntry { meta: RabbitronModule::static_metadata(), external_dll: None },
        ModuleCatalogEntry { meta: TazDesktopModule::static_metadata(), external_dll: None },
        ModuleCatalogEntry { meta: WocketsModule::static_metadata(), external_dll: None },
        ModuleCatalogEntry { meta: YosemiteSamModule::static_metadata(), external_dll: None },
    ];

    if let Some(dir) = plugin_dir {
        if let Ok(rd) = std::fs::read_dir(dir) {
            for entry in rd.flatten() {
                let p = entry.path();
                if p.extension()
                    .and_then(|e| e.to_str())
                    .map(|e| e.eq_ignore_ascii_case("so"))
                    .unwrap_or(false)
                {
                    if let Some(dll_mod) = DynamicModule::load(&p) {
                        entries.push(ModuleCatalogEntry {
                            meta: dll_mod.metadata(),
                            external_dll: Some(p),
                        });
                    }
                }
            }
        }
    }

    entries.sort_by(|a, b| a.meta.name.cmp(&b.meta.name));
    entries
}

/// Instantiate a built-in module by its stable ID.
pub fn create_module_by_id(
    id: &str,
    randomizer_pool: &[String],
    per_module_controls: &BTreeMap<String, [i32; 4]>,
) -> Option<Box<dyn AdModule>> {
    match id {
        "3dbounce" => Some(Box::new(GeoBounceModule::new())),
        "aqua" => Some(Box::new(AquaticRealmModule::new())),
        "blanker" => Some(Box::new(BlankerModule::new())),
        "clocks" => Some(Box::new(ClocksModule::new())),
        "draino" => Some(Box::new(DrainoModule::new())),
        "fade" => Some(Box::new(FadeAwayModule::new())),
        "globe" => Some(Box::new(GlobeModule::new())),
        "grafstat" => Some(Box::new(GraphStatModule::new())),
        "grav" => Some(Box::new(GravityModule::new())),
        "laser" => Some(Box::new(LaserModule::new())),
        "logo" => Some(Box::new(LogoModule::new())),
        "magic" => Some(Box::new(MagicModule::new())),
        "message" => Some(Box::new(MessagesModule::new())),
        "mondrian" => Some(Box::new(MondrianModule::new())),
        "mountain" => Some(Box::new(MountainsModule::new())),
        "nocturne" => Some(Box::new(NocturneModule::new())),
        "punch" => Some(Box::new(PunchOutModule::new())),
        "puzzle" => Some(Box::new(PuzzleModule::new())),
        "rain" => Some(Box::new(HardRainModule::new())),
        "rainstor" => Some(Box::new(RainStormModule::new())),
        "randomizer" => Some(Box::new(RandomizerModule::new(
            randomizer_pool.to_vec(),
            per_module_controls.clone(),
        ))),
        "rose" => Some(Box::new(RoseModule::new())),
        "satori" => Some(Box::new(SatoriModule::new())),
        "sgyra" => Some(Box::new(SpiralGyraModule::new())),
        "shapes" => Some(Box::new(ShapesModule::new())),
        "slidesho" => Some(Box::new(SlideShowModule::new())),
        "spheres" => Some(Box::new(SpheresModule::new())),
        "spotlite" => Some(Box::new(SpotlightModule::new())),
        "stained" => Some(Box::new(StainedGlassModule::new())),
        "starry" => Some(Box::new(StarryNightModule::new())),
        "string" => Some(Box::new(StringTheoryModule::new())),
        "toasters" => Some(Box::new(ToastersModule::new())),
        "vertigo" => Some(Box::new(VertigoModule::new())),
        "warp" => Some(Box::new(WarpModule::new())),
        "worms" => Some(Box::new(WormsModule::new())),
        "wrap" => Some(Box::new(WrapAroundModule::new())),
        "zot" => Some(Box::new(ZotModule::new())),
        // 1994 The Simpsons After Dark 3.0 Pack (15 modules)
        "simpsons_chalkboard" => Some(Box::new(ChalkboardModule::new())),
        "simpsons_files" => Some(Box::new(SimpsonsFilesModule::new())),
        "simpsons_grampa" => Some(Box::new(GrampaModule::new())),
        "simpsons_grass_skirts" => Some(Box::new(GrassSkirtsModule::new())),
        "simpsons_homer_eats" => Some(Box::new(HomerEatsModule::new())),
        "simpsons_how_to_draw" => Some(Box::new(HowToDrawModule::new())),
        "simpsons_itchy_scratchy" => Some(Box::new(ItchyScratchyModule::new())),
        "simpsons_krusty" => Some(Box::new(KrustyModule::new())),
        "simpsons_lisa_moods" => Some(Box::new(LisaMoodsModule::new())),
        "simpsons_mr_burns" => Some(Box::new(MrBurnsModule::new())),
        "simpsons_objets_bart" => Some(Box::new(ObjetsBartModule::new())),
        "simpsons_physics" => Some(Box::new(SimpsonsPhysicsModule::new())),
        "simpsons_clocks" => Some(Box::new(SimpsonsClocksModule::new())),
        "simpsons_trivia" => Some(Box::new(SimpsonsTriviaModule::new())),
        "simpsons_snowball" => Some(Box::new(SnowballModule::new())),
        // 1994 Star Trek: The Next Generation After Dark 3.0 Pack (13 modules)
        "tng_troi" => Some(Box::new(CounselorTroiModule::new())),
        "tng_data_dances" => Some(Box::new(DataDancesModule::new())),
        "tng_encounters" => Some(Box::new(EncountersModule::new())),
        "tng_nanites" => Some(Box::new(NanitesModule::new())),
        "tng_officers_review" => Some(Box::new(OfficersReviewModule::new())),
        "tng_personnel_files" => Some(Box::new(PersonnelFilesModule::new())),
        "tng_science_stations" => Some(Box::new(ScienceStationsModule::new())),
        "tng_starbase" => Some(Box::new(StarbaseModule::new())),
        "tng_starfleet_messages" => Some(Box::new(StarfleetMessagesModule::new())),
        "tng_tachyon" => Some(Box::new(TachyonModule::new())),
        "tng_borg" => Some(Box::new(TheBorgModule::new())),
        "tng_warp_effect" => Some(Box::new(WarpEffectModule::new())),
        "tng_worfs_weapons" => Some(Box::new(WorfsWeaponsModule::new())),
        // 1995 Looney Tunes After Dark 3.0 Pack (12 modules)
        "looney_acme_shopping" => Some(Box::new(AcmeShoppingModule::new())),
        "looney_cartoons_101" => Some(Box::new(Cartoons101Module::new())),
        "looney_conducktor" => Some(Box::new(ConducktorModule::new())),
        "looney_desquetoppe" => Some(Box::new(DesquetoppeModule::new())),
        "looney_marvins_invasion" => Some(Box::new(MarvinsInvasionModule::new())),
        "looney_messages" => Some(Box::new(LooneyMessagesModule::new())),
        "looney_michigan_frog" => Some(Box::new(MichiganFrogModule::new())),
        "looney_putty_tat" => Some(Box::new(PuttyTatModule::new())),
        "looney_rabbitron" => Some(Box::new(RabbitronModule::new())),
        "looney_taz_desktop" => Some(Box::new(TazDesktopModule::new())),
        "looney_wockets" => Some(Box::new(WocketsModule::new())),
        "looney_yosemite_sam" => Some(Box::new(YosemiteSamModule::new())),
        _ => None,
    }
}

/// Instantiate any module from a catalog entry (built-in or dynamic `.so`).
pub fn instantiate_entry(
    entry: &ModuleCatalogEntry,
    randomizer_pool: &[String],
    per_module_controls: &BTreeMap<String, [i32; 4]>,
) -> Option<Box<dyn AdModule>> {
    if let Some(ref dll_path) = entry.external_dll {
        return DynamicModule::load(dll_path).map(|m| Box::new(m) as Box<dyn AdModule>);
    }
    create_module_by_id(&entry.meta.id, randomizer_pool, per_module_controls)
}
