/// Actions dispatched from key/mouse events or internal triggers.
#[derive(Debug, Clone, PartialEq)]
pub enum AppAction {
    Quit,
    QuitAll,
    FocusNext,
    FocusPrev,
    OpenUrl,
    SaveListing,
    DismissListing,
    SnoozeListing,
    AddProfile,
    EditProfile,
    Repoll,
    ShowHelp,
    SelectListing(String),
    SelectProfile(String),
    NavigateUp,
    NavigateDown,
    CycleSort,
    NextImage,
    PrevImage,
}
