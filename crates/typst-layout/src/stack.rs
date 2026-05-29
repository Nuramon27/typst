use typst_library::diag::{SourceResult, bail};
use typst_library::engine::Engine;
use typst_library::foundations::{Content, Packed, Resolve, StyleChain, StyledElem};
use typst_library::introspection::{Locator, SplitLocator};
use typst_library::layout::{
    Abs, AlignElem, Axes, Axis, Dir, FixedAlignment, Fr, Fragment, Frame, HElem, Length,
    Point, Regions, Rel, Size, Spacing, StackChild, StackElem, VElem,
};
use typst_syntax::Span;
use typst_utils::{Get, Numeric};

/// Layout the stack.
#[typst_macros::time(span = elem.span())]
pub fn layout_stack(
    elem: &Packed<StackElem>,
    engine: &mut Engine,
    locator: Locator,
    styles: StyleChain,
    regions: Regions,
) -> SourceResult<Fragment> {
    let mut layouter =
        StackLayouter::new(elem.span(), elem.dir.get(styles), locator, styles, regions);

    let axis = layouter.dir.axis();

    // Spacing to insert before the next block.
    let spacing = elem.spacing.get(styles);
    let mut deferred = None;

    for child in &elem.children {
        match child {
            StackChild::Spacing(kind, minimum) => {
                layouter.layout_spacing(*kind, *minimum);
                deferred = None;
            }
            StackChild::Block(block) => {
                // Transparently handle `h`.
                if let (Axis::X, Some(h)) = (axis, block.to_packed::<HElem>()) {
                    layouter.layout_spacing(h.amount, Rel::zero());
                    deferred = None;
                    continue;
                }

                // Transparently handle `v`.
                if let (Axis::Y, Some(v)) = (axis, block.to_packed::<VElem>()) {
                    layouter.layout_spacing(v.amount, v.minimum.get(styles));
                    deferred = None;
                    continue;
                }

                if let Some(kind) = deferred {
                    layouter.layout_spacing(kind, Rel::zero());
                }

                layouter.layout_block(engine, block, styles)?;
                deferred = spacing;
            }
        }
    }

    layouter.finish()
}

/// Performs stack layout.
struct StackLayouter<'a> {
    /// The span to raise errors at during layout.
    span: Span,
    /// The stacking direction.
    dir: Dir,
    /// The axis of the stacking direction.
    axis: Axis,
    /// Provides unique locations to the stack's children.
    locator: SplitLocator<'a>,
    /// The inherited styles.
    styles: StyleChain<'a>,
    /// The regions to layout children into.
    regions: Regions<'a>,
    /// Whether the stack itself should expand to fill the region.
    expand: Axes<bool>,
    /// The initial size of the current region before we started subtracting.
    initial: Size,
    /// The generic size used by the frames for the current region.
    used: GenericSize<Abs>,
    /// The sum of fractions in the current region.
    fr: Fr,
    /// Already layouted items whose exact positions are not yet known due to
    /// fractional spacing.
    items: Vec<StackItem>,
    /// Finished frames for previous regions.
    finished: Vec<Frame>,
}

/// A prepared item in a stack layout.
enum StackItem {
    /// Absolute spacing between other items.
    Absolute(Abs),
    /// Fractional spacing between other items.
    Fractional(Fr, Abs),
    /// A frame for a layouted block.
    Frame(Frame, Axes<FixedAlignment>),
}

impl<'a> StackLayouter<'a> {
    /// Create a new stack layouter.
    fn new(
        span: Span,
        dir: Dir,
        locator: Locator<'a>,
        styles: StyleChain<'a>,
        mut regions: Regions<'a>,
    ) -> Self {
        let axis = dir.axis();
        let expand = regions.expand;

        // Disable expansion along the block axis for children.
        regions.expand.set(axis, false);

        Self {
            span,
            dir,
            axis,
            locator: locator.split(),
            styles,
            regions,
            expand,
            initial: regions.size,
            used: GenericSize::zero(),
            fr: Fr::zero(),
            items: vec![],
            finished: vec![],
        }
    }

    /// Add spacing along the spacing direction.
    fn layout_spacing(&mut self, spacing: Spacing, minimum: Rel<Length>) {
        match spacing {
            Spacing::Rel(v) => {
                // Resolve the spacing and limit it to the remaining space.
                let resolved = v
                    .resolve(self.styles)
                    .relative_to(self.regions.base().get(self.axis));
                let remaining = self.regions.size.get_mut(self.axis);
                let limited = resolved.min(*remaining);
                if self.dir.axis() == Axis::Y {
                    *remaining -= limited;
                }
                self.used.main += limited;
                self.items.push(StackItem::Absolute(resolved));
            }
            Spacing::Fr(v) => {
                self.fr += v;
                let resolved_minimum = minimum
                    .resolve(self.styles)
                    .relative_to(self.regions.base().get(self.axis));
                let remaining = self.regions.size.get_mut(self.axis);
                let limited = resolved_minimum.min(*remaining);
                if self.dir.axis() == Axis::Y {
                    *remaining -= limited;
                }
                self.used.main += limited;
                self.items.push(StackItem::Fractional(v, resolved_minimum));
            }
        }
    }

    /// Layout an arbitrary block.
    fn layout_block(
        &mut self,
        engine: &mut Engine,
        block: &Content,
        styles: StyleChain,
    ) -> SourceResult<()> {
        if self.regions.is_full() {
            self.finish_region()?;
        }

        // Block-axis alignment of the `AlignElem` is respected by stacks.
        let align = if let Some(align) = block.to_packed::<AlignElem>() {
            align.alignment.get(styles)
        } else if let Some(styled) = block.to_packed::<StyledElem>() {
            styles.chain(&styled.styles).get(AlignElem::alignment)
        } else {
            styles.get(AlignElem::alignment)
        }
        .resolve(styles);

        let fragment = crate::layout_fragment(
            engine,
            block,
            self.locator.next(&block.span()),
            styles,
            self.regions,
        )?;

        let len = fragment.len();
        for (i, frame) in fragment.into_iter().enumerate() {
            // Grow our size, shrink the region and save the frame for later.
            let specific_size = frame.size();
            if self.dir.axis() == Axis::Y {
                self.regions.size.y -= specific_size.y;
            }

            let generic_size = match self.axis {
                Axis::X => GenericSize::new(specific_size.y, specific_size.x),
                Axis::Y => GenericSize::new(specific_size.x, specific_size.y),
            };

            self.used.main += generic_size.main;
            self.used.cross.set_max(generic_size.cross);

            self.items.push(StackItem::Frame(frame, align));

            if i + 1 < len {
                self.finish_region()?;
            }
        }

        Ok(())
    }

    /// Clamp fractional spaces which don't reach their minimum size
    /// to this minimum.
    ///
    /// Returns the sum of minimum-spacing for the remaining fractional
    /// spaces.
    fn clamp_fractional_to_minimum(&mut self, fr_space: Abs) -> Abs {
        let mut cumulated_minimum: Abs = self.items
            .iter()
            .filter_map(|item| if let StackItem::Fractional(_, minimum) = item {
                Some(minimum)
            } else { None })
            .sum();

        let mut recalculation_necessary = true;
        while recalculation_necessary {
            recalculation_necessary = false;
            let mut cumulated_minimum_new = Abs::zero();
            let mut frs_new = Fr::zero();
            for item in &mut self.items {
                match item {
                    StackItem::Fractional(v, minimum) => {
                        let share = v.share(self.fr, fr_space + cumulated_minimum);
                        if share >= *minimum {
                            cumulated_minimum_new += *minimum;
                            frs_new += *v;
                        } else {
                            *item = StackItem::Absolute(*minimum);
                            recalculation_necessary = true;
                        }
                    },
                    _ => ()
                }
            }
            cumulated_minimum = cumulated_minimum_new;
            self.fr = frs_new;
        }

        cumulated_minimum
    }

    /// Advance to the next region.
    fn finish_region(&mut self) -> SourceResult<()> {
        // Determine the size of the stack in this region depending on whether
        // the region expands.
        let mut size = self
            .expand
            .select(self.initial, self.used.into_axes(self.axis))
            .min(self.initial);

        // Expand fully if there are fr spacings.
        let full = self.initial.get(self.axis);
        let remaining = full - self.used.main;
        if self.fr.get() > 0.0 && full.is_finite() {
            self.used.main = full;
            size.set(self.axis, full);
        }

        if !size.is_finite() {
            bail!(self.span, "stack spacing is infinite");
        }

        let cumulated_minimum = self.clamp_fractional_to_minimum(remaining);

        let mut output = Frame::hard(size);
        let mut cursor = Abs::zero();
        let mut ruler: FixedAlignment = self.dir.start().into();

        // Place all frames.
        for item in self.items.drain(..) {
            match item {
                StackItem::Absolute(v) => cursor += v,
                StackItem::Fractional(v, _) => cursor += v.share(self.fr, remaining + cumulated_minimum),
                StackItem::Frame(frame, align) => {
                    if self.dir.is_positive() {
                        ruler = ruler.max(align.get(self.axis));
                    } else {
                        ruler = ruler.min(align.get(self.axis));
                    }

                    // Align along the main axis.
                    let parent = size.get(self.axis);
                    let child = frame.size().get(self.axis);
                    let main = ruler.position(parent - self.used.main)
                        + if self.dir.is_positive() {
                            cursor
                        } else {
                            self.used.main - child - cursor
                        };

                    // Align along the cross axis.
                    let other = self.axis.other();
                    let cross = align
                        .get(other)
                        .position(size.get(other) - frame.size().get(other));

                    let pos = GenericSize::new(cross, main).to_point(self.axis);
                    cursor += child;
                    output.push_frame(pos, frame);
                }
            }
        }

        // Advance to the next region.
        self.regions.next();
        self.initial = self.regions.size;
        self.used = GenericSize::zero();
        self.fr = Fr::zero();
        self.finished.push(output);

        Ok(())
    }

    /// Finish layouting and return the resulting frames.
    fn finish(mut self) -> SourceResult<Fragment> {
        self.finish_region()?;
        Ok(Fragment::frames(self.finished))
    }
}

/// A generic size with main and cross axes. The axes are generic, meaning the
/// main axis could correspond to either the X or the Y axis.
#[derive(Default, Copy, Clone, Eq, PartialEq, Hash)]
struct GenericSize<T> {
    /// The cross component, along the axis perpendicular to the main.
    pub cross: T,
    /// The main component.
    pub main: T,
}

impl<T> GenericSize<T> {
    /// Create a new instance from the two components.
    const fn new(cross: T, main: T) -> Self {
        Self { cross, main }
    }

    /// Convert to the specific representation, given the current main axis.
    fn into_axes(self, main: Axis) -> Axes<T> {
        match main {
            Axis::X => Axes::new(self.main, self.cross),
            Axis::Y => Axes::new(self.cross, self.main),
        }
    }
}

impl GenericSize<Abs> {
    /// The zero value.
    fn zero() -> Self {
        Self { cross: Abs::zero(), main: Abs::zero() }
    }

    /// Convert to a point.
    fn to_point(self, main: Axis) -> Point {
        self.into_axes(main).to_point()
    }
}
