export type UserProfile = {
  id?: string;
  role: string;
  researchField: string;
  focusTopic: string | null;
  readingGoal: string;
  outputLanguage: string;
  experienceLevel: string;
  updatedAt?: string;
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
};

export type ModelConnectionResult = {
  connected: boolean;
  latencyMs: number;
  modelIdentity: string;
  apiType: string;
  endpoint: string;
  statusCode: number | null;
  statusText: string;
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

export type PaperParseStatusResponse = {
  paperId: string;
  parseStatus: string;
  progress: number;
  stage: string;
  errorCode: string | null;
  errorMessage: string | null;
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
  workflowCurrentStep: string;
  nextActionRequired: string | null;
  allowedActions: string[];
  fallbackActions: string[];
  latestHandoffSummaryIds: string[];
  latestAgentRuns: AgentRunSummary[];
  activeRun?: ActiveAgentRun | null;
  updatedAt: string;
};

export type ContextBatch = {
  batchIndex: number;
  sectionIds: string[];
  sectionTitles: string[];
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
  usedHandoffSummaryIds: string[];
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
  quote: string;
  section: string;
  page: number | null;
  locator: string;
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
