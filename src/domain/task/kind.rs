/// The type of analysis a task performs.
///
/// Each variant maps to a specific operation in the analysis pipeline.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum TaskKind {
    // --- Binary inventory ---
    /// Inventory the binary: list all functions, sections, etc.
    InventoryBinary,
    /// Inventory modules within a binary.
    InventoryModules,

    // --- Analysis ---
    /// Analyze a single function's shape, calls, and references.
    AnalyzeFunction,
    /// Analyze all functions in a module.
    AnalyzeModule,
    /// Analyze the call graph for a region.
    AnalyzeCallGraph,
    /// Analyze cross-references for an address or symbol.
    AnalyzeCrossReferences,

    // --- Decompilation ---
    /// Decompile a single function.
    DecompileFunction,
    /// Decompile an entire module.
    DecompileModule,

    // --- Type recovery ---
    /// Recover type information for a function's parameters and locals.
    RecoverTypes,
    /// Recover structure layouts from a binary.
    RecoverStructures,
    /// Recover calling conventions used in a function or module.
    RecoverCallingConventions,

    // --- Verification ---
    /// Verify a previously made claim.
    VerifyClaim,
    /// Verify a set of related claims.
    VerifyClaimSet,
    /// Generate a test or implementation contract based on claims.
    GenerateImplementationContract,
    /// Validate an implementation contract against the binary.
    ValidateImplementationContract,

    // --- Re-implementation ---
    /// Generate C/C++/Rust re-implementation of a function.
    GenerateReimplementation,
    /// Optimize a generated re-implementation.
    OptimizeReimplementation,
    /// Validate a re-implementation against the original binary.
    ValidateReimplementation,

    // --- Campaign management ---
    /// Evaluate campaign status and determine next steps.
    EvaluateCampaign,
    /// Refresh inventory (re-scan binary for new functions).
    RefreshInventory,
    /// Expire stale leases and re-queue tasks.
    ExpireLeases,
    /// Aggregate results from completed tasks.
    AggregateResults,

    // --- Reporting ---
    /// Generate a summary report for a campaign.
    GenerateReport,
    /// Generate a diff report between two analysis runs.
    GenerateDiffReport,

    /// A user-defined or plugin task kind.
    Custom(String),
}
