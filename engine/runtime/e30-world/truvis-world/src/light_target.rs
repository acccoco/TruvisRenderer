use crate::guid_new_type::LightHandle;

/// 灯光身份必须包含所属存储；不同 light map 的 SlotMap key 可以相同。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum LightTarget {
    Point(LightHandle),
    Spot(LightHandle),
    Area(LightHandle),
}
