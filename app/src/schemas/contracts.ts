const agentRunSummarySchema = z.object({
  id: z.string(),
  agentType: z.string(),
  status: z.string(),
  finishedAt: z.string().nullable(),
  summary: z.string().nullable(),
  handoffSummaryId: z.string().nullable(),
  contextPlan: z.lazy(() => contextPlanSchema).nullable().optional(),
});

const contextBatchSchema = z.object({
  batchIndex: z.number(),
  sectionIds: z.array(z.string()),
  sectionTitles: z.array(z.string()),
  carryInSummaryIds: z.array(z.string()),
  promptBudgetEstimate: z.number(),
});

const contextPlanSchema = z.object({
  runtimeMode: z.string(),
  sectionStrategy: z.string(),
  selectionReason: z.string(),
  handoffChainComplete: z.boolean(),
  backfillReason: z.string().nullable(),
  gapCategories: z.array(z.string()),
  selectedSectionIds: z.array(z.string()),
  usedHandoffSummaryIds: z.array(z.string()),
  batchCount: z.number(),
  currentBatchIndex: z.number(),
  truncated: z.boolean(),
  fallbackApplied: z.boolean(),
  batches: z.array(contextBatchSchema),
});

const activeAgentRunSchema = z.object({
  id: z.string(),
  agentType: z.string(),
  status: z.string(),
  currentBatchIndex: z.number(),
  currentBatchCount: z.number(),
  contextPlan: contextPlanSchema,
});

const evidenceItemSchema = z.object({
  quote: z.string(),
  section: z.string(),
  page: z.number().nullable(),
  locator: z.string(),
});

const handoffSummarySchema = z.object({
  id: z.string(),
  runId: z.string(),
  paperId: z.string(),
  agentType: z.string(),
  stage: z.string(),
  compressedConclusion: z.string(),
  keyPoints: z.array(z.string()),
  carryForwardQuestions: z.array(z.string()),
  carryForwardEvidence: z.array(evidenceItemSchema),
  nextStepSuggestion: z.string(),
  generatedAt: z.string(),
});

import { z } from 'zod';

export const paperSearchItemSchema = z.object({
  id: z.string(),
  source: z.string(),
  sourcePaperId: z.string(),
  title: z.string(),
  authors: z.array(z.string()),
  year: z.number().nullable(),
  abstract: z.string().nullable(),
  pdfUrl: z.string().nullable(),
  detailUrl: z.string(),
  hasPdf: z.boolean(),
  venue: z.string(),
});

export const paperSearchResponseSchema = z.object({
  items: z.array(paperSearchItemSchema),
  page: z.number(),
  pageSize: z.number(),
  hasMore: z.boolean(),
  source: z.string(),
});

export const profileResponseSchema = z.object({
  role: z.string(),
  researchField: z.string(),
  focusTopic: z.string().nullable(),
  readingGoal: z.string(),
  outputLanguage: z.string(),
  experienceLevel: z.string(),
  id: z.string(),
  updatedAt: z.string(),
});

export const modelConfigResponseSchema = z.object({
  id: z.string(),
  displayName: z.string(),
  provider: z.string(),
  baseUrl: z.string(),
  modelName: z.string(),
  apiType: z.string().nullable(),
  agentType: z.string().nullable(),
  isDefault: z.boolean(),
  isRecent: z.boolean(),
  hasCredential: z.boolean(),
  updatedAt: z.string(),
});

export const modelConfigDetailResponseSchema = modelConfigResponseSchema.extend({
  apiKey: z.string(),
});

export const modelConfigListResponseSchema = z.object({
  items: z.array(modelConfigResponseSchema),
  recentId: z.string().nullable(),
});

export const modelConnectionResponseSchema = z.object({
  connected: z.boolean(),
  latencyMs: z.number(),
  modelIdentity: z.string(),
  apiType: z.string(),
  endpoint: z.string(),
  statusCode: z.number().nullable(),
  statusText: z.string(),
});

export const importPaperFromFileResponseSchema = z.object({
  paperId: z.string(),
  uploadedFileId: z.string(),
  parseStatus: z.string(),
  metadataNeedsConfirmation: z.boolean(),
});

export const confirmPaperMetadataResponseSchema = z.object({
  paperId: z.string(),
  title: z.string(),
  authors: z.array(z.string()),
  year: z.number().nullable(),
  venue: z.string().nullable(),
  abstract: z.string().nullable(),
});

export const paperParseStatusResponseSchema = z.object({
  paperId: z.string(),
  parseStatus: z.string(),
  progress: z.number(),
  stage: z.string(),
  errorCode: z.string().nullable(),
  errorMessage: z.string().nullable(),
  updatedAt: z.string(),
});

export const readerSnapshotSchema = z.object({
  paperId: z.string(),
  title: z.string(),
  source: z.string(),
  authors: z.array(z.string()),
  abstractText: z.string().nullable(),
  venue: z.string().nullable(),
  year: z.number().nullable(),
  fileName: z.string().nullable(),
  storagePath: z.string().nullable(),
  parseStatus: z.string(),
  parseProgress: z.number(),
  parseErrorCode: z.string().nullable(),
  parseErrorMessage: z.string().nullable(),
  libraryItemId: z.string().nullable(),
  libraryStatus: z.string().nullable(),
  libraryTags: z.array(z.string()),
  starred: z.boolean(),
  uploadedFileId: z.string().nullable(),
  mimeType: z.string().nullable(),
  sizeBytes: z.number().nullable(),
  workflowCurrentStep: z.string(),
  nextActionRequired: z.string().nullable(),
  allowedActions: z.array(z.string()),
  fallbackActions: z.array(z.string()),
  latestHandoffSummaryIds: z.array(z.string()),
  latestAgentRuns: z.array(agentRunSummarySchema),
  activeRun: activeAgentRunSchema.nullable().optional(),
  updatedAt: z.string(),
});

export const runAgentResponseSchema = z.object({
  runId: z.string(),
  status: z.string(),
  pollKey: z.string(),
});

export const agentRunDetailSchema = z.object({
  id: z.string(),
  paperId: z.string(),
  agentType: z.string(),
  status: z.string(),
  inputSnapshot: z.string(),
  outputSnapshot: z.string().nullable(),
  handoffSummary: handoffSummarySchema.nullable().optional(),
  contextPlan: contextPlanSchema.nullable().optional(),
  errorCode: z.string().nullable(),
  errorMessage: z.string().nullable(),
  startedAt: z.string().nullable(),
  finishedAt: z.string().nullable(),
});

export const libraryItemSchema = z.object({
  id: z.string(),
  paperId: z.string(),
  title: z.string(),
  status: z.string(),
  tags: z.array(z.string()),
  starred: z.boolean(),
  updatedAt: z.string(),
});

export const libraryListResponseSchema = z.object({
  items: z.array(libraryItemSchema),
  page: z.number(),
  pageSize: z.number(),
  hasMore: z.boolean(),
});

export const libraryItemMutationResponseSchema = z.object({
  item: libraryItemSchema,
});
