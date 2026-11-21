use crate::{Message, Opcode, fixed_body::fixed_packet};
use binrw::{BinRead, BinWrite};

/// In-game build-path selection. Draft deselection uses a different sentinel.
#[derive(Clone, Copy, Debug, PartialEq, Eq, strum::FromRepr, BinRead, BinWrite)]
#[brw(big, repr = u8)]
#[repr(u8)]
pub enum RecommendedBuildPath {
    Slot0 = 0,
    Slot1 = 1,
    Slot2 = 2,
    NoBuildPath = 0xfe,
}

/// `0x0486 RequestRecommendedBuildPath`: 1-byte client build-path announcement.
#[derive(Clone, Copy, Debug, PartialEq, Eq, BinRead, BinWrite)]
#[brw(big)]
pub struct RequestRecommendedBuildPathBody {
    pub path: RecommendedBuildPath,
}
fixed_packet!(
    RequestRecommendedBuildPathBody,
    Opcode::RequestRecommendedBuildPath,
    1
);

/// `0x0487 SetRecommendedBuildPath`: 5-byte server build-path assignment.
#[derive(Clone, Copy, Debug, PartialEq, Eq, BinRead, BinWrite)]
#[brw(big)]
pub struct SetRecommendedBuildPathBody {
    pub actor: u32,
    pub path: RecommendedBuildPath,
}
fixed_packet!(
    SetRecommendedBuildPathBody,
    Opcode::SetRecommendedBuildPath,
    5
);

impl RecommendedBuildPath {
    pub fn announcement(self) -> Message {
        RequestRecommendedBuildPathBody { path: self }.encode()
    }

    pub fn applied_to(self, actor: u32) -> Message {
        SetRecommendedBuildPathBody { actor, path: self }.encode()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recommended_build_path_bodies_roundtrip_golden_bytes() {
        for path in [
            RecommendedBuildPath::Slot0,
            RecommendedBuildPath::Slot1,
            RecommendedBuildPath::Slot2,
            RecommendedBuildPath::NoBuildPath,
        ] {
            let req = path.announcement();
            assert_eq!(req.body, [path as u8]);
            assert_eq!(
                RequestRecommendedBuildPathBody::read_exact(&req.body)
                    .unwrap()
                    .path,
                path
            );

            let applied = path.applied_to(1502);
            assert_eq!(applied.body, [0, 0, 5, 222, path as u8]);
            let decoded = SetRecommendedBuildPathBody::read_exact(&applied.body).unwrap();
            assert_eq!(decoded.actor, 1502);
            assert_eq!(decoded.path, path);
        }
        assert!(RequestRecommendedBuildPathBody::read_exact(&[3]).is_err());
        assert!(SetRecommendedBuildPathBody::read_exact(&[0, 0, 5, 222, 3]).is_err());
    }
}
