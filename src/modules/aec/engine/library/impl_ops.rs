use super::*;

impl StyleLibrary {
    /// An empty library (no materials, no wall/opening/slab styles).
    pub fn empty() -> Self {
        Self {
            materials: Vec::new(),
            wall_styles: Vec::new(),
            opening_styles: Vec::new(),
            slab_styles: Vec::new(),
            slab_structural_styles: Vec::new(),
            floor_finish_styles: Vec::new(),
            profiles: Vec::new(),
        }
    }

    /// Inserts or replaces (by `id`) a material.
    pub fn upsert_material(&mut self, material: Material) {
        if let Some(existing) = self.materials.iter_mut().find(|m| m.id == material.id) {
            *existing = material;
        } else {
            self.materials.push(material);
        }
    }

    /// Inserts or replaces (by `style.id`) a wall style.
    pub fn upsert_wall_style(&mut self, wall_style: WallStyle) {
        if let Some(existing) = self
            .wall_styles
            .iter_mut()
            .find(|s| s.style.id == wall_style.style.id)
        {
            *existing = wall_style;
        } else {
            self.wall_styles.push(wall_style);
        }
    }

    /// Inserts or replaces (by `style.id`) an opening style.
    pub fn upsert_opening_style(&mut self, opening_style: OpeningStyle) {
        if let Some(existing) = self
            .opening_styles
            .iter_mut()
            .find(|s| s.style.id == opening_style.style.id)
        {
            *existing = opening_style;
        } else {
            self.opening_styles.push(opening_style);
        }
    }

    /// Inserts or replaces (by `style.id`) a slab style.
    pub fn upsert_slab_style(&mut self, slab_style: SlabStyle) {
        if let Some(existing) = self
            .slab_styles
            .iter_mut()
            .find(|s| s.style.id == slab_style.style.id)
        {
            *existing = slab_style;
        } else {
            self.slab_styles.push(slab_style);
        }
    }

    /// Looks up a slab style by id.
    pub fn find_slab_style(&self, id: &str) -> Option<&SlabStyle> {
        self.slab_styles.iter().find(|s| s.style.id == id)
    }

    /// Looks up an opening style by id.
    pub fn find_opening_style(&self, id: &str) -> Option<&OpeningStyle> {
        self.opening_styles.iter().find(|s| s.style.id == id)
    }

    /// Seed style for `kind`, else the first library style of that kind.
    pub fn opening_style_for_kind(
        &self,
        kind: crate::modules::aec::engine::openings::OpeningKind,
    ) -> Option<&OpeningStyle> {
        let id = crate::modules::aec::engine::opening_style::seed_style_id_for_kind(kind);
        self.find_opening_style(id)
            .or_else(|| self.opening_styles.iter().find(|s| s.kind == kind))
    }

    /// Removes a material by `id`. Returns `true` if a material was removed.
    pub fn remove_material(&mut self, id: &str) -> bool {
        let before = self.materials.len();
        self.materials.retain(|m| m.id != id);
        self.materials.len() != before
    }

    /// Removes a wall style by `id`. Returns `true` if a wall style was removed.
    pub fn remove_wall_style(&mut self, id: &str) -> bool {
        let before = self.wall_styles.len();
        self.wall_styles.retain(|s| s.style.id != id);
        self.wall_styles.len() != before
    }

    /// Removes an opening style by `id`. Returns `true` if one was removed.
    pub fn remove_opening_style(&mut self, id: &str) -> bool {
        let before = self.opening_styles.len();
        self.opening_styles.retain(|s| s.style.id != id);
        self.opening_styles.len() != before
    }

    /// Removes a slab style by `id`. Returns `true` if one was removed.
    pub fn remove_slab_style(&mut self, id: &str) -> bool {
        let before = self.slab_styles.len();
        self.slab_styles.retain(|s| s.style.id != id);
        self.slab_styles.len() != before
    }

    /// Inserts or replaces a structural slab style by `style.id`.
    pub fn upsert_slab_structural_style(&mut self, style: SlabStructuralStyle) {
        if let Some(existing) = self
            .slab_structural_styles
            .iter_mut()
            .find(|s| s.style.id == style.style.id)
        {
            *existing = style;
        } else {
            self.slab_structural_styles.push(style);
        }
    }

    /// Looks up a structural slab style by id.
    pub fn find_slab_structural_style(&self, id: &str) -> Option<&SlabStructuralStyle> {
        self.slab_structural_styles.iter().find(|s| s.style.id == id)
    }

    /// Looks up a structural slab style by name or id.
    pub fn find_slab_structural_style_by_name_or_id(&self, name_or_id: &str) -> Option<&SlabStructuralStyle> {
        let trimmed = name_or_id.trim();
        self.slab_structural_styles.iter().find(|s| {
            s.style.id == trimmed
                || s.style.name.eq_ignore_ascii_case(trimmed)
                || trimmed.contains(&s.style.name)
        })
    }

    /// Removes a structural slab style by `id`.
    pub fn remove_slab_structural_style(&mut self, id: &str) -> bool {
        let before = self.slab_structural_styles.len();
        self.slab_structural_styles.retain(|s| s.style.id != id);
        self.slab_structural_styles.len() != before
    }

    /// Inserts or replaces a floor finish style by `style.id`.
    pub fn upsert_floor_finish_style(&mut self, style: FloorFinishStyle) {
        if let Some(existing) = self
            .floor_finish_styles
            .iter_mut()
            .find(|s| s.style.id == style.style.id)
        {
            *existing = style;
        } else {
            self.floor_finish_styles.push(style);
        }
    }

    /// Looks up a floor finish style by id.
    pub fn find_floor_finish_style(&self, id: &str) -> Option<&FloorFinishStyle> {
        self.floor_finish_styles.iter().find(|s| s.style.id == id)
    }

    /// Looks up a floor finish style by name or id.
    pub fn find_floor_finish_style_by_name_or_id(&self, name_or_id: &str) -> Option<&FloorFinishStyle> {
        let trimmed = name_or_id.trim();
        self.floor_finish_styles.iter().find(|s| {
            s.style.id == trimmed
                || s.style.name.eq_ignore_ascii_case(trimmed)
                || trimmed.contains(&s.style.name)
        })
    }

    /// Removes a floor finish style by `id`.
    pub fn remove_floor_finish_style(&mut self, id: &str) -> bool {
        let before = self.floor_finish_styles.len();
        self.floor_finish_styles.retain(|s| s.style.id != id);
        self.floor_finish_styles.len() != before
    }

    /// Inserts or replaces a valid section profile by `style.id`.
    pub fn upsert_section_profile(&mut self, mut profile: SectionProfile) -> bool {
        if !profile.close_custom_contour() {
            return false;
        }
        if let Some(existing) = self
            .profiles
            .iter_mut()
            .find(|existing| existing.style.id == profile.style.id)
        {
            *existing = profile;
        } else {
            self.profiles.push(profile);
        }
        true
    }

    /// Looks up a section profile by id.
    pub fn find_section_profile(&self, id: &str) -> Option<&SectionProfile> {
        self.profiles.iter().find(|profile| profile.style.id == id)
    }

    /// Looks up a section profile by name or id.
    pub fn find_section_profile_by_name_or_id(&self, name_or_id: &str) -> Option<&SectionProfile> {
        let trimmed = name_or_id.trim();
        self.profiles.iter().find(|profile| {
            profile.style.id == trimmed
                || profile.style.name.eq_ignore_ascii_case(trimmed)
                || trimmed.contains(&profile.style.name)
        })
    }

    /// Removes a section profile by id. Returns `true` if one was removed.
    pub fn remove_section_profile(&mut self, id: &str) -> bool {
        let before = self.profiles.len();
        self.profiles.retain(|profile| profile.style.id != id);
        self.profiles.len() != before
    }

    /// Resolves effective slab layers from a slab style, composing layers from its
    /// referenced structural and floor finish sub-styles if needed.
    pub fn resolve_slab_layers(&self, slab_style_id: &str) -> Vec<SlabStyleLayer> {
        if let Some(ss) = self.find_slab_style(slab_style_id) {
            if ss.structural_style_id.is_some() || ss.default_finish_style_id.is_some() {
                let mut layers = Vec::new();
                if let Some(f_id) = &ss.default_finish_style_id {
                    if let Some(fs) = self.find_floor_finish_style(f_id) {
                        layers.extend(fs.layers.clone());
                    }
                }
                if let Some(s_id) = &ss.structural_style_id {
                    if let Some(st) = self.find_slab_structural_style(s_id) {
                        layers.extend(st.layers.clone());
                    }
                }
                if !layers.is_empty() {
                    return layers;
                }
            }
            return ss.layers.clone();
        }
        Vec::new()
    }

    /// Returns all wall styles in a hierarchical tree order: roots first,
    /// each followed immediately by its descendants, siblings sorted by name.
    pub fn wall_style_tree(&self) -> Vec<TreeNode<'_>> {
        let mut children: std::collections::HashMap<&str, Vec<&WallStyle>> =
            std::collections::HashMap::new();
        let mut roots: Vec<&WallStyle> = Vec::new();
        for ws in &self.wall_styles {
            match &ws.style.parent {
                Some(pid) if self.wall_styles.iter().any(|other| &other.style.id == pid) => {
                    children.entry(pid.as_str()).or_default().push(ws);
                }
                _ => roots.push(ws),
            }
        }
        roots.sort_by(|a, b| a.style.name.to_lowercase().cmp(&b.style.name.to_lowercase()));
        for siblings in children.values_mut() {
            siblings.sort_by(|a, b| a.style.name.to_lowercase().cmp(&b.style.name.to_lowercase()));
        }

        let mut ordered = Vec::with_capacity(self.wall_styles.len());
        let mut stack: Vec<(&WallStyle, usize)> = roots
            .into_iter()
            .rev()
            .map(|ws| (ws, 0))
            .collect();
        while let Some((ws, depth)) = stack.pop() {
            ordered.push(TreeNode { style: ws, depth });
            if let Some(kids) = children.get(ws.style.id.as_str()) {
                for kid in kids.iter().rev() {
                    stack.push((kid, depth + 1));
                }
            }
        }
        ordered
    }

    /// Hierarchical opening-style order: roots first, then descendants.
    pub fn opening_style_tree(&self) -> Vec<OpeningTreeNode<'_>> {
        let mut children: std::collections::HashMap<&str, Vec<&OpeningStyle>> =
            std::collections::HashMap::new();
        let mut roots: Vec<&OpeningStyle> = Vec::new();
        for os in &self.opening_styles {
            match &os.style.parent {
                Some(pid) if self.opening_styles.iter().any(|other| &other.style.id == pid) => {
                    children.entry(pid.as_str()).or_default().push(os);
                }
                _ => roots.push(os),
            }
        }
        roots.sort_by(|a, b| a.style.name.to_lowercase().cmp(&b.style.name.to_lowercase()));
        for siblings in children.values_mut() {
            siblings.sort_by(|a, b| a.style.name.to_lowercase().cmp(&b.style.name.to_lowercase()));
        }

        let mut ordered = Vec::with_capacity(self.opening_styles.len());
        let mut stack: Vec<(&OpeningStyle, usize)> = roots
            .into_iter()
            .rev()
            .map(|os| (os, 0))
            .collect();
        while let Some((os, depth)) = stack.pop() {
            ordered.push(OpeningTreeNode { style: os, depth });
            if let Some(kids) = children.get(os.style.id.as_str()) {
                for kid in kids.iter().rev() {
                    stack.push((kid, depth + 1));
                }
            }
        }
        ordered
    }

    /// Hierarchical slab-style order: roots first, then descendants.
    pub fn slab_style_tree(&self) -> Vec<SlabTreeNode<'_>> {
        let mut children: std::collections::HashMap<&str, Vec<&SlabStyle>> =
            std::collections::HashMap::new();
        let mut roots: Vec<&SlabStyle> = Vec::new();
        for ss in &self.slab_styles {
            match &ss.style.parent {
                Some(pid) if self.slab_styles.iter().any(|other| &other.style.id == pid) => {
                    children.entry(pid.as_str()).or_default().push(ss);
                }
                _ => roots.push(ss),
            }
        }
        roots.sort_by(|a, b| a.style.name.to_lowercase().cmp(&b.style.name.to_lowercase()));
        for siblings in children.values_mut() {
            siblings.sort_by(|a, b| a.style.name.to_lowercase().cmp(&b.style.name.to_lowercase()));
        }

        let mut ordered = Vec::with_capacity(self.slab_styles.len());
        let mut stack: Vec<(&SlabStyle, usize)> = roots
            .into_iter()
            .rev()
            .map(|ss| (ss, 0))
            .collect();
        while let Some((ss, depth)) = stack.pop() {
            ordered.push(SlabTreeNode { style: ss, depth });
            if let Some(kids) = children.get(ss.style.id.as_str()) {
                for kid in kids.iter().rev() {
                    stack.push((kid, depth + 1));
                }
            }
        }
        ordered
    }

    /// Returns every wall style / layer index that references `material_id`.
    pub fn materials_using(&self, material_id: &str) -> Vec<(&WallStyle, usize)> {
        let mut result = Vec::new();
        for ws in &self.wall_styles {
            for (idx, layer) in ws.layers.iter().enumerate() {
                if layer.material_id == material_id {
                    result.push((ws, idx));
                }
            }
        }
        result
    }
}

