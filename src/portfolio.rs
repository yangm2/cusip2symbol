use std::collections::BTreeMap;

/// A node in the portfolio tree. Interior nodes have children; leaf nodes have a ticker + allocation.
#[derive(Debug)]
pub enum PortfolioNode {
    Group {
        name: String,
        children: Vec<PortfolioNode>,
    },
    Holding {
        ticker: String,
        allocation: Allocation,
    },
}

#[derive(Debug, Clone, Copy)]
pub enum Allocation {
    Usd(f64),
    Pct(f64),
}

/// Parsed + normalized portfolio ready for exposure calculation.
#[derive(Debug)]
pub struct Portfolio {
    pub root: Vec<PortfolioNode>,
    pub total_usd: f64,
}

/// Result of exposure calculation for a single holding.
#[derive(Debug)]
pub struct HoldingExposure {
    pub ticker: String,
    pub portfolio_weight: f64, // fraction of total portfolio (0..1)
    pub holding_pct: Option<f64>, // pctVal from N-PORT (already in %)
    pub exposure_pct: f64, // portfolio_weight * holding_pct / 100
}

/// Exposure result for a subtree.
#[derive(Debug)]
pub struct ExposureNode {
    pub name: String,
    pub exposure_pct: f64,
    pub children: Vec<ExposureResult>,
}

#[derive(Debug)]
pub enum ExposureResult {
    Group(ExposureNode),
    Leaf(HoldingExposure),
}

pub fn parse_portfolio(toml_str: &str) -> Result<Portfolio, Box<dyn std::error::Error>> {
    let table: BTreeMap<String, toml::Value> = toml::from_str(toml_str)?;
    let mut root = Vec::new();
    let mut total_usd = 0.0;

    for (key, value) in &table {
        let node = parse_node(key, value, &mut total_usd)?;
        root.push(node);
    }

    Ok(Portfolio { root, total_usd })
}

fn parse_node(
    name: &str,
    value: &toml::Value,
    total_usd: &mut f64,
) -> Result<PortfolioNode, Box<dyn std::error::Error>> {
    match value {
        toml::Value::Table(map) => {
            // Check if this table contains leaf values (strings) or subtables
            let has_subtables = map.values().any(|v| v.is_table());
            let has_leaves = map.values().any(|v| v.is_str());

            if has_subtables && has_leaves {
                // Mixed: split into group children and leaf holdings
                let mut children = Vec::new();
                for (k, v) in map {
                    children.push(parse_node(k, v, total_usd)?);
                }
                Ok(PortfolioNode::Group {
                    name: name.to_string(),
                    children,
                })
            } else if has_subtables {
                let mut children = Vec::new();
                for (k, v) in map {
                    children.push(parse_node(k, v, total_usd)?);
                }
                Ok(PortfolioNode::Group {
                    name: name.to_string(),
                    children,
                })
            } else {
                // All leaves — this is a group of holdings
                let mut children = Vec::new();
                for (ticker, alloc_val) in map {
                    let alloc = parse_allocation(
                        alloc_val.as_str().ok_or_else(|| {
                            format!("Expected string allocation for {ticker}")
                        })?,
                    )?;
                    if let Allocation::Usd(usd) = alloc {
                        *total_usd += usd;
                    }
                    children.push(PortfolioNode::Holding {
                        ticker: ticker.clone(),
                        allocation: alloc,
                    });
                }
                Ok(PortfolioNode::Group {
                    name: name.to_string(),
                    children,
                })
            }
        }
        toml::Value::String(s) => {
            let alloc = parse_allocation(s)?;
            if let Allocation::Usd(usd) = alloc {
                *total_usd += usd;
            }
            Ok(PortfolioNode::Holding {
                ticker: name.to_string(),
                allocation: alloc,
            })
        }
        _ => Err(format!("Unexpected value type for '{name}'").into()),
    }
}

fn parse_allocation(s: &str) -> Result<Allocation, Box<dyn std::error::Error>> {
    let s = s.trim();
    if let Some(rest) = s.strip_prefix('$') {
        let val: f64 = rest.replace(',', "").parse()?;
        Ok(Allocation::Usd(val))
    } else if let Some(rest) = s.strip_suffix('%') {
        let val: f64 = rest.trim().parse()?;
        Ok(Allocation::Pct(val))
    } else {
        Err(format!("Invalid allocation '{s}': use '$N' for USD or 'N%' for percentage").into())
    }
}

impl Portfolio {
    /// Get the portfolio weight (0..1) for a given allocation.
    pub fn weight(&self, alloc: Allocation) -> f64 {
        match alloc {
            Allocation::Usd(usd) => {
                if self.total_usd > 0.0 {
                    usd / self.total_usd
                } else {
                    0.0
                }
            }
            Allocation::Pct(pct) => pct / 100.0,
        }
    }

    /// Collect all leaf tickers with their portfolio weights.
    pub fn leaf_tickers(&self) -> Vec<(String, f64)> {
        let mut result = Vec::new();
        for node in &self.root {
            collect_leaves(node, self, &mut result);
        }
        result
    }
}

fn collect_leaves(node: &PortfolioNode, portfolio: &Portfolio, out: &mut Vec<(String, f64)>) {
    match node {
        PortfolioNode::Holding { ticker, allocation } => {
            out.push((ticker.clone(), portfolio.weight(*allocation)));
        }
        PortfolioNode::Group { children, .. } => {
            for child in children {
                collect_leaves(child, portfolio, out);
            }
        }
    }
}

/// Build exposure results tree given a map of ticker -> (name, holding_pct).
pub fn build_exposure_tree(
    nodes: &[PortfolioNode],
    portfolio: &Portfolio,
    holding_pcts: &BTreeMap<String, (String, f64)>,
) -> Vec<ExposureResult> {
    let mut results = Vec::new();
    for node in nodes {
        results.push(build_exposure_node(node, portfolio, holding_pcts));
    }
    results
}

fn build_exposure_node(
    node: &PortfolioNode,
    portfolio: &Portfolio,
    holding_pcts: &BTreeMap<String, (String, f64)>,
) -> ExposureResult {
    match node {
        PortfolioNode::Holding { ticker, allocation } => {
            let weight = portfolio.weight(*allocation);
            let (holding_pct, exposure) = if let Some((_name, pct)) = holding_pcts.get(ticker) {
                (Some(*pct), weight * pct / 100.0)
            } else {
                (None, 0.0)
            };
            ExposureResult::Leaf(HoldingExposure {
                ticker: ticker.clone(),
                portfolio_weight: weight,
                holding_pct,
                exposure_pct: exposure * 100.0, // convert to percentage
            })
        }
        PortfolioNode::Group { name, children } => {
            let child_results = build_exposure_tree(children, portfolio, holding_pcts);
            let total_exposure: f64 = child_results
                .iter()
                .map(|r| match r {
                    ExposureResult::Group(g) => g.exposure_pct,
                    ExposureResult::Leaf(l) => l.exposure_pct,
                })
                .sum();
            ExposureResult::Group(ExposureNode {
                name: name.clone(),
                exposure_pct: total_exposure,
                children: child_results,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_portfolio_usd() {
        let toml = r#"
[Equities]
IVV = "$100000"
QQQ = "$50000"

[Bonds]
AGG = "$50000"
"#;
        let p = parse_portfolio(toml).unwrap();
        assert!((p.total_usd - 200000.0).abs() < 0.01);
        let leaves = p.leaf_tickers();
        assert_eq!(leaves.len(), 3);

        let ivv = leaves.iter().find(|(t, _)| t == "IVV").unwrap();
        assert!((ivv.1 - 0.5).abs() < 0.01); // $100k / $200k
    }

    #[test]
    fn test_parse_portfolio_pct() {
        let toml = r#"
[Equities]
IVV = "60%"
AGG = "40%"
"#;
        let p = parse_portfolio(toml).unwrap();
        let leaves = p.leaf_tickers();
        let ivv = leaves.iter().find(|(t, _)| t == "IVV").unwrap();
        assert!((ivv.1 - 0.6).abs() < 0.01);
    }

    #[test]
    fn test_parse_portfolio_nested() {
        let toml = r#"
["US Equities"."Large Cap"]
IVV = "$100000"
QQQ = "$50000"

["US Equities"."Small Cap"]
VB = "$25000"

[Bonds]
AGG = "$75000"
"#;
        let p = parse_portfolio(toml).unwrap();
        assert!((p.total_usd - 250000.0).abs() < 0.01);
        let leaves = p.leaf_tickers();
        assert_eq!(leaves.len(), 4);
    }

    #[test]
    fn test_build_exposure_tree() {
        let toml = r#"
[Equities]
IVV = "$100000"
QQQ = "$100000"

[Bonds]
AGG = "$100000"
"#;
        let p = parse_portfolio(toml).unwrap();
        let mut pcts = BTreeMap::new();
        // IVV holds 7% NVDA, QQQ holds 9% NVDA, AGG holds 0%
        pcts.insert("IVV".to_string(), ("NVIDIA CORP".to_string(), 7.0));
        pcts.insert("QQQ".to_string(), ("NVIDIA CORP".to_string(), 9.0));

        let tree = build_exposure_tree(&p.root, &p, &pcts);
        // Total exposure = (1/3 * 7/100 + 1/3 * 9/100) * 100 = 5.33%
        let total: f64 = tree
            .iter()
            .map(|r| match r {
                ExposureResult::Group(g) => g.exposure_pct,
                ExposureResult::Leaf(l) => l.exposure_pct,
            })
            .sum();
        assert!((total - 5.333).abs() < 0.1);
    }
}
