//! SpawnableOutlineBehaviour: the pooled rectangles CatalogBehaviour.SpawnableOutline shows while a contraption is held at the cursor.

use super::contraption::RotatedRectangle;

/// One pooled SpriteRenderer child: active, `size`, localPosition and the z euler angle.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct OutlineChild {
    pub active: bool,
    pub size: [f32; 2],
    pub position: [f32; 2],
    pub angle: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SpawnableOutline {
    /// SpawnableOutlineBehaviour.children
    pub children: Vec<OutlineChild>,
    /// CatalogBehaviour.SpawnableOutline.gameObject.activeInHierarchy
    pub active: bool,
    /// transform.position (Update follows the mouse) and transform.localScale.x (-1 flipped).
    pub position: [f32; 2],
    pub flip: f32,
}

impl Default for SpawnableOutline {
    fn default() -> Self {
        Self::new()
    }
}

impl SpawnableOutline {
    /// SpawnableOutlineBehaviour.Awake (`Instance = this`) and the field initialiser: no children.
    pub fn new() -> Self {
        SpawnableOutline { children: Vec::new(), active: false, position: [0.0, 0.0], flip: 1.0 }
    }

    /// SpawnableOutlineBehaviour.SetOutline: the existing children take the rectangles in order and the surplus are deactivated; missing children are appended.
    pub fn SetOutline(&mut self, rectangles: &[RotatedRectangle]) {
        // ContraptionOutline.op_Equality (1.28.45 first returns when CurrentOutline == outline) is Equals(right).
        // ContraptionOutline.Equals: object.Equals on the Rectangles arrays, reference identity. Equal only when
        // DragTool's paste passes the clipboard's Outline again after the Clipboard constructor's SetOutline of that same array,
        // with no other SetOutline between; SetOutline is the children's only writer, so they already hold those rectangles and
        // going on again leaves the same state as the early return.
        let num = rectangles.len();
        let count = self.children.len();
        let mut num2 = 0;
        for child in &mut self.children {
            if num2 >= num {
                child.active = false;
                continue;
            }
            let r = rectangles[num2];
            *child = OutlineChild { active: true, size: r.Size, position: r.Center, angle: r.AngleDegrees };
            num2 += 1;
        }
        if count < num {
            for r in &rectangles[count..] {
                self.children.push(OutlineChild { active: true, size: r.Size, position: r.Center, angle: r.AngleDegrees });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(x: f32) -> RotatedRectangle {
        RotatedRectangle { Center: [x, 0.0], Size: [1.0, 2.0], AngleDegrees: 30.0 }
    }

    #[test]
    fn reuses_children_and_hides_the_rest() {
        let mut o = SpawnableOutline::new();
        o.SetOutline(&[r(1.0), r(2.0), r(3.0)]);
        assert_eq!(o.children.len(), 3);
        o.SetOutline(&[r(5.0)]);
        assert_eq!(o.children.iter().map(|c| c.active).collect::<Vec<_>>(), [true, false, false]);
        assert_eq!(o.children[0].position, [5.0, 0.0]);
        o.SetOutline(&[r(1.0), r(2.0), r(3.0), r(4.0)]);
        assert!(o.children.iter().all(|c| c.active) && o.children.len() == 4);
    }
}
