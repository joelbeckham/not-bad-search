use crate::Hit;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SortMode { #[default] PreferNewer, NewestFirst, OldestFirst }
impl SortMode {
    pub fn label(self)->&'static str { match self {Self::PreferNewer=>"Prefer newer",Self::NewestFirst=>"Newest first",Self::OldestFirst=>"Oldest first"} }
    pub fn by_date(self)->bool {self!=Self::PreferNewer}
    pub fn sql_order(self)->&'static str {match self {Self::OldestFirst=>"sort_date IS NULL,sort_date ASC,path",_=>"sort_date IS NULL,sort_date DESC,path"}}
}
/// Sort a relevance-ranked candidate set. Recency contributes at most 15%,
/// decaying by half every 180 days; exact names retain priority in Prefer newer.
pub fn sort(hits:&mut Vec<Hit>,query:&str,mode:SortMode,now:i64) {
    if mode.by_date() {
        hits.sort_by(|a,b| match (a.timestamp,b.timestamp) {
            (Some(a),Some(b))=>if mode==SortMode::NewestFirst {b.cmp(&a)}else{a.cmp(&b)},
            (Some(_),None)=>std::cmp::Ordering::Less,(None,Some(_))=>std::cmp::Ordering::Greater,_=>std::cmp::Ordering::Equal,
        }.then(a.path.cmp(&b.path)));
    } else {
        let mut ranked:Vec<_>=hits.drain(..).enumerate().map(|(i,hit)| {
            let boost=hit.timestamp.map(|t| {let days=now.saturating_sub(t).max(0) as f64/86400.; 0.15*2f64.powf(-days/180.)}).unwrap_or(0.);
            (hit,(1.+boost)/(60.+i as f64))
        }).collect();
        let exact=query.trim().to_lowercase();
        ranked.sort_by(|a,b| (b.0.name.to_lowercase()==exact).cmp(&(a.0.name.to_lowercase()==exact)).then(b.1.total_cmp(&a.1)).then(a.0.path.cmp(&b.0.path)));
        hits.extend(ranked.into_iter().map(|(hit,_)|hit));
    }
}
