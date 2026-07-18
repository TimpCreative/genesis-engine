//! Manual diagnostics for terrain-quality investigation (not part of CI).
//!
//! Run: `cargo test --release -p genesis_tectonics terrain_diagnostics -- --ignored --nocapture`

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, VecDeque};

    use genesis_core::HexId;
    use genesis_core::parameters::WorldParameters;
    use genesis_core::time::WorldYear;

    use crate::boundary::{BoundaryClass, ConvergentSubtype, detect_and_classify_boundaries};
    use crate::history::generate_full_history_with_tectonics;
    use crate::plate::TectonicsState;
    use crate::plate_surface::continental_crust_at;

    #[test]
    #[ignore = "manual terrain diagnostics"]
    fn terrain_diagnostics_at_one_billion_years() {
        let target_year = std::env::var("GENESIS_TARGET_YEAR")
            .ok()
            .and_then(|s| s.parse::<i64>().ok())
            .unwrap_or(1_000_000_000);
        let mut params = WorldParameters::default();
        params.core.grid.subdivision_level = 7;
        // GENESIS_VALIDATION_SEED overrides the seed so 4B-year claims can be
        // sampled across realizations instead of one chaotic trajectory.
        if let Some(seed) = std::env::var("GENESIS_VALIDATION_SEED")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
        {
            params.core.seed = genesis_core::WorldSeed::from_integer(seed);
        }
        let mut world = genesis_core::create_world(params).expect("world");
        let mut state = TectonicsState::new();
        generate_full_history_with_tectonics(
            &mut world,
            &mut state,
            WorldYear(target_year),
            |_| {},
        )
        .expect("history");

        let data = &world.data;
        let grid = &data.grid;
        let n = data.cell_count() as usize;
        let sea = data.sea_level_m;

        let boundaries = detect_and_classify_boundaries(data, &state.registry, &state.projection);

        // ---------- 1. Boundary edge totals ----------
        let mut edge_counts: BTreeMap<&'static str, u64> = BTreeMap::new();
        for edges in boundaries.edges.values() {
            for edge in edges {
                let key = match edge.class {
                    BoundaryClass::Divergent => "divergent",
                    BoundaryClass::Transform => "transform",
                    BoundaryClass::Convergent(s) => match s {
                        ConvergentSubtype::ContinentalContinental => "convergent_CC",
                        ConvergentSubtype::ContinentalOceanic => "convergent_CO",
                        ConvergentSubtype::OceanicOceanic => "convergent_OO",
                    },
                };
                *edge_counts.entry(key).or_insert(0) += 1;
            }
        }
        eprintln!("=== boundary edge counts (directed) ===");
        for (k, v) in &edge_counts {
            eprintln!("{k}: {v}");
        }

        // ---------- 2. Below-sea connected components ----------
        let below_sea: Vec<bool> = (0..n).map(|i| data.elevation_mean[i] < sea).collect();
        let mut comp_of = vec![usize::MAX; n];
        let mut components: Vec<Vec<usize>> = Vec::new();
        for start in 0..n {
            if !below_sea[start] || comp_of[start] != usize::MAX {
                continue;
            }
            let id = components.len();
            let mut comp = Vec::new();
            let mut queue = VecDeque::from([start]);
            comp_of[start] = id;
            while let Some(i) = queue.pop_front() {
                comp.push(i);
                for nb in grid.neighbors(HexId(i as u32)) {
                    let j = nb.0 as usize;
                    if j < n && below_sea[j] && comp_of[j] == usize::MAX {
                        comp_of[j] = id;
                        queue.push_back(j);
                    }
                }
            }
            components.push(comp);
        }
        components.sort_by_key(|c| std::cmp::Reverse(c.len()));
        let total_below: usize = components.iter().map(|c| c.len()).sum();
        eprintln!(
            "=== below-sea components: {} total hexes below sea ===",
            total_below
        );
        let main_ocean = components.first().map(|c| c.len()).unwrap_or(0);
        eprintln!("main ocean component: {main_ocean} hexes");
        let inland: Vec<&Vec<usize>> = components.iter().skip(1).collect();
        eprintln!(
            "detached sub-sea components (candidate inland seas): {}",
            inland.len()
        );
        for comp in inland.iter().take(20) {
            let mut min_e = f32::MAX;
            let mut cont = 0u64;
            for &i in comp.iter() {
                min_e = min_e.min(data.elevation_mean[i]);
                if continental_crust_at(data, &state.registry, &state.projection, HexId(i as u32)) {
                    cont += 1;
                }
            }
            eprintln!(
                "  size={:4} deepest={:8.1}m continental_crust={}/{}",
                comp.len(),
                min_e,
                cont,
                comp.len()
            );
        }

        // ---------- 3. Land elevation histogram ----------
        let bands = [
            ("0-10m", 0.0f32, 10.0f32),
            ("10-50m", 10.0, 50.0),
            ("50-150m", 50.0, 150.0),
            ("150-550m", 150.0, 550.0),
            ("550-1500m", 550.0, 1500.0),
            (">1500m", 1500.0, f32::MAX),
        ];
        let mut band_counts = [0u64; 6];
        let mut land_total = 0u64;
        #[allow(clippy::needless_range_loop)]
        for i in 0..n {
            if below_sea[i] {
                continue;
            }
            land_total += 1;
            let above = data.elevation_mean[i] - sea;
            for (bi, (_, lo, hi)) in bands.iter().enumerate() {
                if above >= *lo && above < *hi {
                    band_counts[bi] += 1;
                    break;
                }
            }
        }
        eprintln!("=== land elevation bands (land total {land_total}) ===");
        for (bi, (name, _, _)) in bands.iter().enumerate() {
            eprintln!(
                "{name:>10}: {:5} ({:.1}%)",
                band_counts[bi],
                100.0 * band_counts[bi] as f64 / land_total as f64
            );
        }

        // ---------- 4. Mountain hexes: coast proximity + nearest boundary ----------
        // Ring distance to nearest below-sea hex (BFS from all ocean, land only).
        let mut ocean_dist = vec![u32::MAX; n];
        let mut queue = VecDeque::new();
        for i in 0..n {
            if below_sea[i] {
                ocean_dist[i] = 0;
                queue.push_back(i);
            }
        }
        while let Some(i) = queue.pop_front() {
            let d = ocean_dist[i];
            if d >= 8 {
                continue;
            }
            for nb in grid.neighbors(HexId(i as u32)) {
                let j = nb.0 as usize;
                if j < n && !below_sea[j] && ocean_dist[j] == u32::MAX {
                    ocean_dist[j] = d + 1;
                    queue.push_back(j);
                }
            }
        }

        // Ring distance to nearest boundary hex, and which classes seen there.
        let boundary_set: Vec<bool> = {
            let mut v = vec![false; n];
            for &h in &boundaries.boundary_hexes {
                v[h.0 as usize] = true;
            }
            v
        };
        let mut bdist = vec![u32::MAX; n];
        let mut bqueue = VecDeque::new();
        for &h in &boundaries.boundary_hexes {
            bdist[h.0 as usize] = 0;
            bqueue.push_back(h.0 as usize);
        }
        while let Some(i) = bqueue.pop_front() {
            let d = bdist[i];
            if d >= 8 {
                continue;
            }
            for nb in grid.neighbors(HexId(i as u32)) {
                let j = nb.0 as usize;
                if j < n && bdist[j] == u32::MAX {
                    bdist[j] = d + 1;
                    bqueue.push_back(j);
                }
            }
        }

        // Classify each mountain hex by nearest boundary hex's dominant class.
        let mut mountain_stats: BTreeMap<String, u64> = BTreeMap::new();
        let mut mountain_total = 0u64;
        for i in 0..n {
            if below_sea[i] || data.elevation_mean[i] <= 3000.0 {
                continue;
            }
            mountain_total += 1;
            let coast = match ocean_dist[i] {
                0..=2 => "coastal(<=2 rings)",
                3..=4 => "near-coast(3-4)",
                _ => "interior(5+)",
            };
            // Nearest boundary hexes within its ring distance: collect classes at
            // the closest ring.
            let mut class_key = "no-boundary(8+)".to_string();
            if bdist[i] <= 8 {
                // find any boundary hex at ring == bdist[i]: sample classes of
                // boundary hexes at that distance via local search bounded by
                // ring count (cheap: rescan boundary list neighborhood).
                let mut found: Option<&'static str> = None;
                let mut seen = vec![false; n];
                seen[i] = true;
                let mut ring_sizes = VecDeque::from([(i, 0u32)]);
                while let Some((cur, cd)) = ring_sizes.pop_front() {
                    if cd > bdist[i] || cd > 8 {
                        break;
                    }
                    #[allow(clippy::collapsible_if)]
                    if boundary_set[cur] && cd == bdist[i] {
                        if let Some(edges) = boundaries.edges.get(&HexId(cur as u32)) {
                            // Dominant: first convergent edge, else first class.
                            let mut label = None;
                            for e in edges {
                                if let BoundaryClass::Convergent(s) = e.class {
                                    label = Some(match s {
                                        ConvergentSubtype::ContinentalContinental => "CC",
                                        ConvergentSubtype::ContinentalOceanic => "CO",
                                        ConvergentSubtype::OceanicOceanic => "OO",
                                    });
                                    break;
                                }
                            }
                            if label.is_none() {
                                label = Some(match edges[0].class {
                                    BoundaryClass::Divergent => "div",
                                    BoundaryClass::Transform => "trans",
                                    BoundaryClass::Convergent(_) => unreachable!(),
                                });
                            }
                            found = label;
                            break;
                        }
                    }
                    for nb in grid.neighbors(HexId(cur as u32)) {
                        let j = nb.0 as usize;
                        if j < n && !seen[j] {
                            seen[j] = true;
                            ring_sizes.push_back((j, cd + 1));
                        }
                    }
                }
                if let Some(l) = found {
                    class_key = format!("near-{l}({} rings)", bdist[i]);
                }
            }
            *mountain_stats
                .entry(format!("{coast} | {class_key}"))
                .or_insert(0) += 1;
        }
        eprintln!("=== mountain hexes (>3000m): {mountain_total} ===");
        for (k, v) in &mountain_stats {
            eprintln!("{k}: {v}");
        }

        // ---------- 5. Coastline vs convergent proximity ----------
        let mut coastline = 0u64;
        let mut coastline_near_convergent = 0u64;
        for i in 0..n {
            if below_sea[i] || ocean_dist[i] != 1 {
                continue;
            }
            coastline += 1;
            // convergent boundary within 2 rings?
            if bdist[i] <= 2 {
                // check the nearest boundary hex has a convergent edge
                let mut q = VecDeque::from([(i, 0u32)]);
                let mut seen = vec![false; n];
                seen[i] = true;
                let mut is_conv = false;
                'outer: while let Some((cur, cd)) = q.pop_front() {
                    if cd > 2 {
                        break;
                    }
                    #[allow(clippy::collapsible_if)]
                    if boundary_set[cur] {
                        if let Some(edges) = boundaries.edges.get(&HexId(cur as u32)) {
                            if edges
                                .iter()
                                .any(|e| matches!(e.class, BoundaryClass::Convergent(_)))
                            {
                                is_conv = true;
                                break 'outer;
                            }
                        }
                    }
                    for nb in grid.neighbors(HexId(cur as u32)) {
                        let j = nb.0 as usize;
                        if j < n && !seen[j] {
                            seen[j] = true;
                            q.push_back((j, cd + 1));
                        }
                    }
                }
                if is_conv {
                    coastline_near_convergent += 1;
                }
            }
        }
        eprintln!("=== coastline ===");
        eprintln!("coastline hexes: {coastline}");
        eprintln!(
            "coastline within 2 rings of convergent boundary: {coastline_near_convergent} ({:.1}%)",
            100.0 * coastline_near_convergent as f64 / coastline.max(1) as f64
        );

        // ---------- 6. §11 #10-12 Wilson-cycle summary ----------
        let land_fraction = crate::validation::continental_fraction(data);
        let comps = crate::validation::below_sea_components(data);
        // Detached = sub-open-ocean bodies (§5.8 trapped-basin definition);
        // a connected water body ≥1% of cells is a real secondary ocean.
        let open_min =
            (data.cell_count() as f64 * crate::accretion::OPEN_OCEAN_MIN_FRACTION).ceil() as usize;
        let detached: Vec<(usize, f32)> =
            comps.iter().filter(|c| c.0 < open_min).copied().collect();
        let detached_cells: usize = detached.iter().map(|c| c.0).sum();
        let detached_fraction = detached_cells as f32 / data.cell_count() as f32;
        let deepest_detached = detached.iter().map(|c| c.1).fold(f32::MAX, f32::min);
        let passive = crate::validation::passive_margin_fraction(data, &state);
        let mut continental_cells = 0usize;
        for i in 0..n {
            if continental_crust_at(data, &state.registry, &state.projection, HexId(i as u32)) {
                continental_cells += 1;
            }
        }
        eprintln!("=== §11 #10-12 Wilson-cycle summary ===");
        eprintln!(
            "plates: {} (target 5-15) | continental crust area: {:.1}% of sphere",
            state.registry.count(),
            100.0 * continental_cells as f64 / n as f64
        );
        eprintln!(
            "#10 land fraction: {:.1}% (target 20-45%)",
            land_fraction * 100.0
        );
        eprintln!(
            "#11 detached below-sea: {:.2}% of cells ({} components, target <2%)",
            detached_fraction * 100.0,
            detached.len()
        );
        if detached.is_empty() {
            eprintln!("#11 deepest detached component: none (target >= -6000m)");
        } else {
            eprintln!("#11 deepest detached component: {deepest_detached:.0}m (target >= -6000m)");
        }
        eprintln!(
            "#12 passive-margin coastline: {:.1}% (target >=25%)",
            passive * 100.0
        );

        // ---------- 7. Plate speed distribution (§2.4) ----------
        let radius_km = data.parameters.core.planet.radius_km;
        let mut speeds: Vec<f64> = state
            .registry
            .iter()
            .map(|p| p.motion_rate_rad_per_year * radius_km * 1e5)
            .collect();
        speeds.sort_by(f64::total_cmp);
        if !speeds.is_empty() {
            eprintln!(
                "plate speeds cm/yr: min {:.1} | median {:.1} | max {:.1} (expect: most 1-4, slab-rich 6-15, none >18)",
                speeds[0],
                speeds[speeds.len() / 2],
                speeds[speeds.len() - 1]
            );
        }

        eprintln!("=== done ===");
    }
}
