//! Explicit development sandbox destinations. These are never public game requests.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EditorSceneState {
    Cockpit,
    InteriorDeck1,
    InteriorDeck2,
    StationHangar,
}
impl EditorSceneState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Cockpit => "Cockpit",
            Self::InteriorDeck1 => "Interior · Deck 1",
            Self::InteriorDeck2 => "Interior · Deck 2",
            Self::StationHangar => "Station Hangar",
        }
    }
}
