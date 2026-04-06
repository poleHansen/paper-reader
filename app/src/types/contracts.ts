export type UserProfile = {
  id?: string;
  role: string;
  researchField: string;
  focusTopic: string | null;
  readingGoal: string;
  outputLanguage: string;
  experienceLevel: string;
  githubRepoOwner: string | null;
  githubRepoName: string | null;
  githubRepoBranch: string | null;
  githubRepoPathPrefix: string | null;
  githubCdnBaseUrl: string | null;
  githubToken?: string | null;
  hasGithubToken?: boolean;
  updatedAt?: string;
};

export type GitHubUploadTestResponse = {
  publicUrl: string;
  repositoryPath: string;
  message: string;
};

export type SearchPapersRequest = {
  query: string;
  source: string;
  page: number;
  pageSize: number;
};

export type PaperSearchResult = {
  id: string;
  source: string;
  sourcePaperId: string;
  title: string;
  authors: string[];
  year: number | null;
  abstract: string | null;
  pdfUrl: string | null;
  detailUrl: string;
  hasPdf: boolean;
  venue: string;
};

export type SearchPapersResponse = {
  items: PaperSearchResult[];
  page: number;
  pageSize: number;
  hasMore: boolean;
  source: string;
};

export type ModelConfigRequest = {
  displayName: string;
  provider: string;
  baseUrl: string;
  modelName: string;
  apiKey: string;
  apiType: string | null;
  imageInputFormat: string | null;
  agentType: string | null;
  isDefault: boolean;
};

export type UpdateModelConfigRequest = ModelConfigRequest & {
  id: string;
};

export type ModelConfigResponse = {
  id: string;
  displayName: string;
  provider: string;
  baseUrl: string;
  modelName: string;
  apiType: string | null;
  imageInputFormat: string | null;
  agentType: string | null;
  isDefault: boolean;
  isRecent: boolean;
  hasCredential: boolean;
  updatedAt: string;
};

export type ModelConfigDetailResponse = ModelConfigResponse & {
  apiKey: string;
};

export type ModelConfigListResponse = {
  items: ModelConfigResponse[];
  recentId: string | null;
};

export type SelectModelConfigRequest = {
  id: string;
};

export type DeleteModelConfigRequest = {
  id: string;
};

export type TestModelConnectionRequest = {
  provider: string;
  baseUrl: string;
  modelName: string;
  apiKey: string;
  apiType: string | null;
  imageInputFormat: string | null;
};

export type ModelConnectionResult = {
  connected: boolean;
  latencyMs: number;
  modelIdentity: string;
  apiType: string;
  endpoint: string;
  statusCode: number | null;
  statusText: string;
  imageInputSupported: boolean;
  imageInputMessage: string;
  imageInputWorkingFormat: string | null;
  imageProbeAttemptedFormats: string[];
};

export type VisualDiagnostic = {
  scope: string;
  code: string;
  message: string;
  retryable: boolean;
};

export type TestModelConnectionResponse = ModelConnectionResult;

export type ImportPaperFromFileRequest = {
  filePath: string;
};

export type ImportPaperFromLinkRequest = {
  url: string;
  fileName?: string | null;
};

export type ImportPaperFromFileResponse = {
  paperId: string;
  uploadedFileId: string;
  parseStatus: string;
  metadataNeedsConfirmation: boolean;
};

export type ConfirmPaperMetadataRequest = {
  paperId: string;
  title: string;
  authors: string[];
  year: number | null;
  venue: string | null;
  abstract: string | null;
};

export type ConfirmPaperMetadataResponse = {
  paperId: string;
  title: string;
  authors: string[];
  year: number | null;
  venue: string | null;
  abstract: string | null;
};

export type GetPaperParseStatusRequest = {
  paperId: string;
};

export type VisualParsingSummary = {
  enabled: boolean;
  figureCount: number;
  tableCount: number;
  cropSuccessCount: number;
  cropFailedCount: number;
  warnings: string[];
};

export type PaperParseStatusResponse = {
  paperId: string;
  parseStatus: string;
  progress: number;
  stage: string;
  errorCode: string | null;
  errorMessage: string | null;
  visualParsing?: VisualParsingSummary | null;
  updatedAt: string;
};

export type GetReaderSnapshotRequest = {
  paperId: string;
};

export type ReaderSnapshot = {
  paperId: string;
  title: string;
  source: string;
  authors: string[];
  abstractText: string | null;
  venue: string | null;
  year: number | null;
  fileName: string | null;
  storagePath: string | null;
  parseStatus: string;
  parseProgress: number;
  parseErrorCode: string | null;
  parseErrorMessage: string | null;
  libraryItemId: string | null;
  libraryStatus: string | null;
  libraryTags: string[];
  starred: boolean;
  uploadedFileId: string | null;
  mimeType: string | null;
  sizeBytes: number | null;
  parsedContent?: ParsedContentSummary | null;
  workflowCurrentStep: string;
  nextActionRequired: string | null;
  allowedActions: string[];
  fallbackActions: string[];
  latestHandoffSummaryIds: string[];
  latestAgentRuns: AgentRunSummary[];
  activeRun?: ActiveAgentRun | null;
  updatedAt: string;
};

export type ParsedContentSummary = {
  version: number;
  storagePath: string;
  fullTextAvailable: boolean;
  sectionCount: number;
  figureCount: number;
  tableCount: number;
  visualEnabled: boolean;
  visualMode: string;
  visualSummaryCount: number;
  cropSuccessCount: number;
  cropFailedCount: number;
  sampleCaption: string | null;
  sampleSummary: string | null;
  visualWarnings: string[];
  githubUploadDiagnostics: VisualDiagnostic[];
  visualDiagnostics: VisualDiagnostic[];
};

export type ContextBatch = {
  batchIndex: number;
  sectionIds: string[];
  sectionTitles: string[];
  figureIds: string[];
  tableIds: string[];
  carryInSummaryIds: string[];
  promptBudgetEstimate: number;
};

export type ContextPlan = {
  runtimeMode: string;
  sectionStrategy: string;
  selectionReason: string;
  handoffChainComplete: boolean;
  backfillReason: string | null;
  gapCategories: string[];
  selectedSectionIds: string[];
  selectedFigureIds: string[];
  selectedTableIds: string[];
  usedHandoffSummaryIds: string[];
  visualMode: string;
  batchCount: number;
  currentBatchIndex: number;
  truncated: boolean;
  fallbackApplied: boolean;
  batches: ContextBatch[];
};

export type ActiveAgentRun = {
  id: string;
  agentType: string;
  status: string;
  currentBatchIndex: number;
  currentBatchCount: number;
  contextPlan: ContextPlan;
  stageState?: StageState | null;
  actionHistory: ActionHistoryItem[];
};

export type AgentRunSummary = {
  id: string;
  agentType: string;
  status: string;
  finishedAt: string | null;
  summary: string | null;
  handoffSummaryId: string | null;
  contextPlan?: ContextPlan | null;
};

export type EvidenceItem = {
  sourceType: string;
  sourceObjectId: string | null;
  quote: string;
  section: string;
  page: number | null;
  locator: string;
};

export type StageCheckItem = {
  id: string;
  label: string;
  status: string;
  required: boolean;
  evidenceSourceIds: string[];
  note: string | null;
};

export type StageState = {
  stage: string;
  goal: string;
  allowedActions: string[];
  checks: StageCheckItem[];
  visitedSources: string[];
  openQuestions: string[];
  iteration: number;
  maxIterations: number;
  enough: boolean;
};

export type ActionHistoryItem = {
  iteration: number;
  decision: Record<string, unknown>;
  resolvedAction: Record<string, unknown>;
  outputSummary?: unknown;
};

export type ParsedBoundingBox = {
  x: number;
  y: number;
  width: number;
  height: number;
};

export type ParsedObjectMention = {
  sectionId: string | null;
  page: number;
  locator: string;
  sentence: string;
};

export type ParsedFigure = {
  id: string;
  label: string;
  title: string | null;
  caption: string;
  page: number | null;
  sectionId: string | null;
  locator: string;
  imagePath: string | null;
  thumbnailPath: string | null;
  ocrText: string[];
  summary: string | null;
  confidence: number | null;
  boundingBox: ParsedBoundingBox | null;
  captionBoundingBox: ParsedBoundingBox | null;
  mentions: ParsedObjectMention[];
};

export type ParsedTable = {
  id: string;
  label: string;
  title: string | null;
  caption: string;
  page: number | null;
  sectionId: string | null;
  locator: string;
  imagePath: string | null;
  thumbnailPath: string | null;
  ocrText: string[];
  markdownTable: string | null;
  summary: string | null;
  confidence: number | null;
  boundingBox: ParsedBoundingBox | null;
  captionBoundingBox: ParsedBoundingBox | null;
  mentions: ParsedObjectMention[];
};

export type ParsedVisualEvidence = {
  id: string;
  sourceObjectId: string;
  sourceObjectType: string;
  claim: string;
  supportLevel: string;
  evidenceText: string;
  page: number | null;
  locator: string;
  confidence: number | null;
};

export type GetPaperVisualArtifactsRequest = {
  paperId: string;
};

export type PaperVisualArtifactsResponse = {
  paperId: string;
  version: number;
  figures: ParsedFigure[];
  tables: ParsedTable[];
  visualEvidence: ParsedVisualEvidence[];
  githubUploadDiagnostics: VisualDiagnostic[];
  visualDiagnostics: VisualDiagnostic[];
};

export type HandoffSummary = {
  id: string;
  runId: string;
  paperId: string;
  agentType: string;
  stage: string;
  compressedConclusion: string;
  keyPoints: string[];
  carryForwardQuestions: string[];
  carryForwardEvidence: EvidenceItem[];
  nextStepSuggestion: string;
  generatedAt: string;
};

export type RunAgentRequest = {
  paperId: string;
  agentType: string;
  userQuestion?: string | null;
  force?: boolean | null;
  sourceRunIds?: string[] | null;
  sourceHandoffSummaryIds?: string[] | null;
  runtimeMode?: string | null;
  sectionStrategy?: string | null;
  maxSectionsPerBatch?: number | null;
  maxBatches?: number | null;
  pinnedSectionIds?: string[] | null;
  visualMode?: string | null;
  pinnedFigureIds?: string[] | null;
  pinnedTableIds?: string[] | null;
};

export type VisualAnalysisTarget = {
  objectId: string;
  objectType: string;
};

export type VisualAnalysisEvidence = {
  sourceObjectId: string;
  sourceObjectType: string;
  claim: string;
  evidenceText: string;
  page: number | null;
  locator: string;
  confidence: number | null;
};

export type VisualAnalysisItem = {
  objectId: string;
  objectType: string;
  label: string;
  title: string | null;
  page: number | null;
  locator: string;
  stage: string;
  chartType: string | null;
  multimodalSummary: string | null;
  keyFindings: string[];
  evidence: VisualAnalysisEvidence[];
  warnings: string[];
  confidence: number | null;
};

export type AnalyzeVisualsRequest = {
  paperId: string;
  stage: string;
  userQuestion?: string | null;
  force?: boolean | null;
  targetObjectIds?: string[] | null;
  targetObjectTypes?: string[] | null;
  maxItems?: number | null;
};

export type AnalyzeVisualsResponse = {
  paperId: string;
  stage: string;
  visualMode: string;
  targets: VisualAnalysisTarget[];
  analyses: VisualAnalysisItem[];
  warnings: string[];
};

export type RunAgentResponse = {
  runId: string;
  status: string;
  pollKey: string;
};

export type GetAgentRunRequest = {
  runId: string;
};

export type AgentRunDetail = {
  id: string;
  paperId: string;
  agentType: string;
  status: string;
  inputSnapshot: string;
  outputSnapshot: string | null;
  handoffSummary?: HandoffSummary | null;
  contextPlan?: ContextPlan | null;
  stageState?: StageState | null;
  actionHistory: ActionHistoryItem[];
  errorCode: string | null;
  errorMessage: string | null;
  startedAt: string | null;
  finishedAt: string | null;
};

export type SaveToLibraryRequest = {
  paperId: string;
  status?: string | null;
  tags?: string[] | null;
  starred?: boolean | null;
};

export type UpdateLibraryItemRequest = {
  id: string;
  status?: string | null;
  tags?: string[] | null;
  starred?: boolean | null;
};

export type LibraryItem = {
  id: string;
  paperId: string;
  title: string;
  status: string;
  tags: string[];
  starred: boolean;
  updatedAt: string;
};

export type LibraryListRequest = {
  page: number;
  pageSize: number;
  status?: string;
  keyword?: string;
};

export type LibraryListResponse = {
  items: LibraryItem[];
  page: number;
  pageSize: number;
  hasMore: boolean;
};

export type LibraryItemMutationResponse = {
  item: LibraryItem;
};
