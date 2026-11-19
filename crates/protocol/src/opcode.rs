macro_rules! opcodes {
    ($( $name:ident = $raw:literal, )*) => {
        /// Opcodes with observed CE meanings. Unknown numbers stay in Message::opcode.
        #[derive(Debug, Clone, Copy, Eq, PartialEq)]
        #[repr(u16)]
        pub enum Opcode { $( $name = $raw, )* }

        impl TryFrom<u16> for Opcode {
            type Error = u16;
            fn try_from(raw: u16) -> Result<Self, Self::Error> {
                match raw { $( $raw => Ok(Self::$name), )* other => Err(other) }
            }
        }
    };
}

opcodes! {
    ClockSync = 0x0000,
    Hello = 0x0005,
    Heartbeat = 0x0006,
    PlayerUuid = 0x03e8,
    GameSetup = 0x03e9,
    EntityActA = 0x03ea,
    EntityActB = 0x03eb,
    PosUpdate = 0x03ec,
    PlayerHandle = 0x03ed,
    PlayerInfo = 0x03ee,
    TickSyncA = 0x03ef,
    TickSyncB = 0x03f0,
    TickSyncC = 0x03f1,
    Snapshot = 0x03f2,
    EntitySpawn = 0x03f3,
    MoveCmd = 0x03f4,
    StateUpdA = 0x03f5,
    StateUpdB = 0x03f6,
    StateUpdC = 0x03f7,
    EntityFloat = 0x03f8,
    EntityAct2 = 0x03f9,
    StatModifier = 0x041c,
    EntityStat = 0x041d,
    HealthImpact = 0x041e,
    EntityState = 0x042b,
    Position = 0x042e,
    ActorDeath = 0x0430,
    ShopRequest = 0x0439,
    ItemGrant = 0x043d,
    EntityProp = 0x043e,
    EntityData = 0x043f,
    RemoveItem = 0x044b,
    HeroCatalog = 0x0453,
    GameMode = 0x0454,
    HeroLock = 0x0455,
    SetupA = 0x0458,
    Snapshot5V5 = 0x0459,
    SnapshotAral = 0x045a,
    HeroSelection = 0x045e,
    SkinSelection = 0x045f,
    LockSelection = 0x0463,
    SetupC = 0x046b,
    ModeName = 0x046f,
}
