use std::fmt::{self, Debug, Formatter};
use std::iter::FromIterator;

use crate::foundations::{Content, cast, Dict, elem};
use crate::layout::{Dir, Length, Rel, Spacing};

/// Arranges content and spacing horizontally or vertically.
///
/// The stack places a list of items along an axis, with optional spacing
/// between each item.
///
/// # Example
/// ```example
/// #stack(
///   dir: ttb,
///   rect(width: 40pt),
///   rect(width: 120pt),
///   rect(width: 90pt),
/// )
/// ```
///
/// # Accessibility
/// Stacks do not carry any special semantics. The contents of the stack are
/// read by Assistive Technology (AT) in the order in which they have been
/// passed to this function.
#[elem]
pub struct StackElem {
    /// The direction along which the items are stacked. Possible values are:
    ///
    /// - `{ltr}`: Left to right.
    /// - `{rtl}`: Right to left.
    /// - `{ttb}`: Top to bottom.
    /// - `{btt}`: Bottom to top.
    ///
    /// You can use the `start` and `end` methods to obtain the initial and
    /// final points (respectively) of a direction, as `alignment`. You can also
    /// use the `axis` method to determine whether a direction is
    /// `{"horizontal"}` or `{"vertical"}`. The `inv` method returns a
    /// direction's inverse direction.
    ///
    /// For example, `{ttb.start()}` is `top`, `{ttb.end()}` is `bottom`,
    /// `{ttb.axis()}` is `{"vertical"}` and `{ttb.inv()}` is equal to `btt`.
    #[default(Dir::TTB)]
    pub dir: Dir,

    /// Spacing to insert between items where no explicit spacing was provided.
    pub spacing: Option<Spacing>,

    /// The children to stack along the axis.
    #[variadic]
    pub children: Vec<StackChild>,
}

/// A child of a stack element.
#[derive(Clone, PartialEq, Hash)]
pub enum StackChild {
    /// Spacing between other children.
    Spacing(Spacing, Rel<Length>),
    /// Arbitrary block-level content.
    Block(Content),
}

impl Debug for StackChild {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match self {
            Self::Spacing(spacing, minimum) => {
                f.debug_map()
                    .entry(&"spacing", spacing)
                    .entry(&"minimum", minimum)
                    .finish()
                },
            Self::Block(block) => block.fmt(f),
        }
    }
}

cast! {
    StackChild,
    self => match self {
        Self::Spacing(spacing, minimum) => Dict::from_iter([
            ("spacing".into(), spacing.into_value()),
            ("minimum".into(), minimum.into_value()
        )]).into_value(),
        Self::Block(content) => content.into_value(),
    },
    v: Spacing => Self::Spacing(v, Rel::zero()),
    v: Content => Self::Block(v),
}
