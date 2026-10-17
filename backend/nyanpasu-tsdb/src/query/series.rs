//! Grouped sums (§8.1). One scan serves several group requests, each with its own filters;
//! rankings, for example, ignore the filter on their own dimension. Group keys are
//! `Total`, `Labels` and `Mapped`.
