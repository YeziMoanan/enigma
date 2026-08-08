#[derive(Clone, Copy)]
pub struct BattleCatalog {
    game_data: &'static config::GameDB,
}

impl BattleCatalog {
    pub fn new(game_data: &'static config::GameDB) -> Self {
        Self { game_data }
    }

    pub(crate) fn game_data(self) -> &'static config::GameDB {
        self.game_data
    }
}

impl std::fmt::Debug for BattleCatalog {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("BattleCatalog")
    }
}
