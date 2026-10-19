//! Tools and loads a model holds: which node is which, and which show.
//!
//! The art build parents boxy props to the hands and names them (contract:
//! `docs/art/peasant.md`): `Tool_<role>` for the tool of a work role (every
//! node whose name starts with it: `Tool_chip` and `Tool_chip_l`), `Load_<resource
//! id>` for each kind of cargo. All start hidden; at most one tool or one
//! load shows at a time, the one the clip being played handles.

use af_core::prelude::*;
use bevy::prelude::*;

use super::clips::Clip;

/// A work role's tool.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Tool {
    Forage,
    Farm,
    Chop,
    Mine,
    Dig,
    Chip,
    Smash,
    /// Building and repairing.
    Build,
    /// The weapon, while fighting.
    Attack,
}

impl Tool {
    pub const ALL: [Tool; 9] = [Tool::Forage, Tool::Farm, Tool::Chop, Tool::Mine, Tool::Dig, Tool::Chip, Tool::Smash, Tool::Build, Tool::Attack];

    /// The `<role>` of its `Tool_<role>` nodes.
    pub fn role(self) -> &'static str {
        match self {
            Tool::Forage => "forage",
            Tool::Farm => "farm",
            Tool::Chop => "chop",
            Tool::Mine => "mine",
            Tool::Dig => "dig",
            Tool::Chip => "chip",
            Tool::Smash => "smash",
            Tool::Build => "build",
            Tool::Attack => "attack",
        }
    }

    pub fn for_style(style: WorkStyle) -> Tool {
        match style {
            WorkStyle::Forage => Tool::Forage,
            WorkStyle::Farm => Tool::Farm,
            WorkStyle::Chop => Tool::Chop,
            WorkStyle::Mine => Tool::Mine,
            WorkStyle::Dig => Tool::Dig,
            WorkStyle::Chip => Tool::Chip,
            WorkStyle::Smash => Tool::Smash,
        }
    }

    /// The tool a clip works with: the work clips theirs, building and
    /// repairing the hammer, fighting the weapon; `None` for the rest.
    pub fn for_clip(clip: Clip) -> Option<Tool> {
        Some(match clip {
            Clip::Forage => Tool::Forage,
            Clip::Farm => Tool::Farm,
            Clip::Chop => Tool::Chop,
            Clip::Mine => Tool::Mine,
            Clip::Dig => Tool::Dig,
            Clip::Chip => Tool::Chip,
            Clip::Smash => Tool::Smash,
            Clip::Build | Clip::Repair => Tool::Build,
            Clip::Attack | Clip::CombatIdle => Tool::Attack,
            _ => return None,
        })
    }
}

/// What a named node of a model is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Prop {
    Tool(Tool),
    Load(ResourceId),
    /// Named like a prop but for no role or resource the game has: never shown.
    Unknown,
}

/// The prop a node named `name` is, or `None` for any other node.
/// `resource` looks up a resource id.
pub fn classify(name: &str, resource: impl Fn(&str) -> Option<ResourceId>) -> Option<Prop> {
    if let Some(rest) = name.strip_prefix("Tool_") {
        // The longest role the name starts with (none is a prefix of another today).
        let tool = Tool::ALL.into_iter().filter(|t| rest.starts_with(t.role())).max_by_key(|t| t.role().len());
        return Some(tool.map_or(Prop::Unknown, Prop::Tool));
    }
    let rest = name.strip_prefix("Load_")?;
    // Blender suffixes a duplicate `.001`; the id is what comes before.
    let id = rest.split('.').next().unwrap_or(rest);
    Some(resource(id).map_or(Prop::Unknown, Prop::Load))
}

/// Which props show: at most one tool or one load.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Shown {
    pub tool: Option<Tool>,
    pub load: Option<ResourceId>,
}

impl Shown {
    pub fn shows(&self, prop: Prop) -> bool {
        match prop {
            Prop::Tool(t) => self.tool == Some(t),
            Prop::Load(r) => self.load == Some(r),
            Prop::Unknown => false,
        }
    }
}

/// The props to show, following what the unit is animated doing (`base`,
/// its looping state as the animator chose it, before any fallback for a
/// clip the model lacks):
///
/// * the weapon mid-swing (`swinging`) and between swings (CombatIdle);
/// * at work, that work's tool (the load filling up meanwhile stays out of
///   sight);
/// * else the load in the hands (`load`: carried, or being put down);
/// * else the tool of the work being walked to (`heading`, from the front
///   order), or nothing.
pub fn shown_props(base: Clip, swinging: bool, load: Option<ResourceId>, heading: Option<Tool>) -> Shown {
    if swinging {
        return Shown { tool: Some(Tool::Attack), load: None };
    }
    if let Some(tool) = Tool::for_clip(base) {
        return Shown { tool: Some(tool), load: None };
    }
    match load {
        Some(load) => Shown { tool: None, load: Some(load) },
        None => Shown { tool: heading, load: None },
    }
}

/// The tool for the work a front order leads to: gathering in the style of
/// its target (`style_of`), building or repairing. `None` for anything else.
pub fn heading_tool(order: Option<&Order>, style_of: impl Fn(Entity) -> Option<WorkStyle>) -> Option<Tool> {
    match order? {
        Order::Gather { target } => style_of(*target).map(Tool::for_style),
        Order::Build { .. } | Order::Repair { .. } => Some(Tool::Build),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn res(id: &str) -> Option<ResourceId> {
        ["food", "wood", "stone"].iter().position(|r| *r == id).map(|i| ResourceId(i as u8))
    }

    #[test]
    fn nodes_are_classified_by_name() {
        assert_eq!(classify("Tool_chop", res), Some(Prop::Tool(Tool::Chop)));
        assert_eq!(classify("Tool_chip", res), Some(Prop::Tool(Tool::Chip)));
        assert_eq!(classify("Tool_chip_l", res), Some(Prop::Tool(Tool::Chip)), "prefix match");
        assert_eq!(classify("Tool_attack", res), Some(Prop::Tool(Tool::Attack)));
        assert_eq!(classify("Tool_lute", res), Some(Prop::Unknown));
        assert_eq!(classify("Load_wood", res), Some(Prop::Load(ResourceId(1))));
        assert_eq!(classify("Load_wood.001", res), Some(Prop::Load(ResourceId(1))));
        assert_eq!(classify("Load_gold", res), Some(Prop::Unknown));
        for other in ["Team_sash", "mixamorig:RightHand", "char1", "tool_chop", "Loader"] {
            assert_eq!(classify(other, res), None, "{other}");
        }
    }

    #[test]
    fn the_props_follow_the_clip() {
        let wood = Some(ResourceId(1));
        let axe = Shown { tool: Some(Tool::Chop), load: None };
        assert_eq!(shown_props(Clip::Chop, false, None, None), axe);
        assert_eq!(shown_props(Clip::Chop, false, wood, Some(Tool::Chop)), axe, "the load filling up while chopping stays hidden");
        assert_eq!(shown_props(Clip::CarryWalk, false, wood, Some(Tool::Chop)), Shown { tool: None, load: wood }, "carrying home");
        assert_eq!(shown_props(Clip::CarryIdle, false, wood, None), Shown { tool: None, load: wood });
        assert_eq!(shown_props(Clip::Walk, false, wood, None).load, wood, "being put down (Drop)");
        assert_eq!(shown_props(Clip::CombatIdle, false, None, Some(Tool::Chop)).tool, Some(Tool::Attack));
        assert_eq!(shown_props(Clip::CarryWalk, true, wood, None), Shown { tool: Some(Tool::Attack), load: None }, "mid-swing");
        assert_eq!(shown_props(Clip::Build, false, None, None).tool, Some(Tool::Build));
        assert_eq!(shown_props(Clip::Repair, false, None, None).tool, Some(Tool::Build));
        assert_eq!(shown_props(Clip::Walk, false, None, Some(Tool::Mine)).tool, Some(Tool::Mine), "walking to work");
        assert_eq!(shown_props(Clip::Idle, false, None, None), Shown::default());
        assert_eq!(shown_props(Clip::Heal, false, None, None), Shown::default());
        for style in WorkStyle::ALL {
            assert_eq!(Tool::for_clip(Clip::work(style)), Some(Tool::for_style(style)), "{style:?}");
        }

        let shown = shown_props(Clip::Chop, false, None, None);
        assert!(shown.shows(Prop::Tool(Tool::Chop)));
        assert!(!shown.shows(Prop::Tool(Tool::Mine)) && !shown.shows(Prop::Load(ResourceId(1))) && !shown.shows(Prop::Unknown));
    }

    #[test]
    fn the_front_order_names_the_tool_walked_to() {
        let tree = Entity::from_raw_u32(3).unwrap();
        let style = |e: Entity| (e == tree).then_some(WorkStyle::Chop);
        assert_eq!(heading_tool(Some(&Order::Gather { target: tree }), style), Some(Tool::Chop));
        assert_eq!(heading_tool(Some(&Order::Gather { target: Entity::from_raw_u32(4).unwrap() }), style), None, "gone");
        assert_eq!(heading_tool(Some(&Order::Build { site: tree }), style), Some(Tool::Build));
        assert_eq!(heading_tool(Some(&Order::Repair { target: tree }), style), Some(Tool::Build));
        assert_eq!(heading_tool(Some(&Order::ReturnCargo), style), None);
        assert_eq!(heading_tool(Some(&Order::Move { target: Vec2::ZERO }), style), None);
        assert_eq!(heading_tool(None, style), None);
        assert_eq!(Tool::for_style(WorkStyle::Farm).role(), "farm");
    }
}
