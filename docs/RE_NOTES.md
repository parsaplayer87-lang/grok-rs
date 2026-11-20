# Grok reverse-engineering notes

Evidence-driven analysis of the official Grok web client, performed with the REA MCP
server. No proprietary code, assets, branding, or credentials are copied into this
project. What is reproduced here is the *observable interface contract* (route names,
field names, stream semantics) so that an independent client can interoperate.

## Method

| Step | Tool | Result |
| --- | --- | --- |
| Launch CDP target | Edge 157 headless, `--remote-debugging-port=9222` | `Edg/157.0.4341.60`, protocol 1.3 |
| Passive capture #1 | `rea.inspect_web_page` | evidence `ev_03bfa724579ae402a94ff4e52550e2ff25cdb9d0907e3f5599919eb504a7e3d0` |
| Passive capture #2 (live reload, 30 s window) | `rea.inspect_web_page` | evidence `ev_e000cd48b179594455f7397ab89a275ed9f80d008776951a14a86e8a8ccc9dba` |
| Bundle acquisition | 157 chunks from `cdn.grok.com` | 11,479,997 bytes, 0 failures |
| Contract extraction | route + payload + model mining | 533 distinct routes |

Capture #1 attached after load, so REA correctly reported `attach_limited` /
`policy_filtered` and returned no network records. Capture #2 was taken across a
cache-bypassing `Page.reload`, which yielded 245 network records. This distinction
matters: the first capture's silence is a coverage limit, not evidence of absence.

## Observed live endpoints (capture #2)

```
GET  200  https://grok.com/api/auth/session
GET  401  https://grok.com/rest/automations
POST 200  https://grok.com/api/auth/oauth/id-token/nonce
POST 200  https://grok.com/_data/v1/exp/exposures      (experiment exposure)
POST 200  https://grok.com/ht/v1/identify              (Hightouch)
POST 200  https://grok.com/mp/track                    (Mixpanel)
POST 200  https://grok.com/api/log_metric
POST 200  https://grok.com/monitoring                  (Sentry envelope)
POST 204  https://grok.com/cdn-cgi/rum                 (Cloudflare RUM)
```

Chat routes were not exercised because the session was unauthenticated. Their shape
was instead recovered statically from the generated API client chunk
`390pxo7uldcbg.js` (219,315 bytes).

## Model identifiers present in the bundle

```
grok-4-1                              grok-4-auto
grok-4-1-non-thinking-w-tool          grok-4-1-non-thinking-no-tool-1111b
grok-4-mini-thinking-tahoe            grok-4-fast-side-by-side
grok-3                                grok-3-reasoning
grok-3-mini-companion                 grok-420
```

Feature-flag strings observed alongside them: `deepSearchSupportsTrace`,
`deepsearch_suggestion_pill`, `deepsearch_workspace_id`, `grok-4-home-promotion`,
`grok-4-promotion-banner-id`.

## Chat request contract

`POST /rest/app-chat/conversations/new` — `Content-Type: application/json`.
Body fields, in the order the client serialises them:

```
systemPromptName, temporary, modelName, message,
fileAttachments, imageAttachments,
disableSearch, enableImageGeneration, returnImageBytes,
returnRawGrokInXaiRequest, enableImageStreaming, imageGenerationCount,
forceConcise, toolOverrides, enableSideBySide, sendFinalMetadata,
customInstructions, customPersonality, deepsearchPreset,
imageEditUri, isReasoning, webpageUrls, imageEditUris, workspaceIds,
disableTextFollowUps, disableArtifact, responseMetadata, disableArtifactDiff,
doForceTriggerArtifact, isFromGrokFiles,
selectedFileTextContent, selectedFileTextContentStartPosition,
selectedFileTextContentEndPosition,
disableMemory, forceSideBySide, modelsUserCanUse, modelMode,
skipResponseCache, isAsyncChat, enableNsfw, isKidsMode,
supportedFastTools, isRegenRequest, templateId,
disableSelfHarmShortCircuit, collectionIds, connectorId
```

Conversation creation is a separate call, `POST /rest/app-chat/conversations`, with
`{systemPromptName, temporary, addResponseRequest, workspaceIds, taskResultId,
templateId, kind}`. So "new conversation + first response" is a compound operation,
not a single resource.

## Response model (the streamed unit)

```
responseId, conversationId, message, sender, createTime,
parentResponseId, partial,
query, queryType, webSearchResults, citedWebSearchResults,
xpostIds, generatedImageUrls, imageAttachments, fileAttachments,
cardAttachmentsJson, thinkingTrace, steps, imageEditUri, error,
webpageUrls, metadata, memoryReferences, uiLayout,
ragResults, citedRagResults, connectorSearchResults, collectionSearchResults
```

Key structural facts:

- **Responses form a tree, not a list.** `parentResponseId` plus a separate
  `{responseId, sender, parentResponseId, threadParentId}` node type means editing a
  message or regenerating creates a *sibling branch*, and the UI walks to a leaf.
  `/rest/app-chat/conversations/{id}/response-node` and `/model-responses` /
  `/user-responses` exist to traverse it.
- **`partial`** marks an in-flight response; the client renders progressively.
- **`thinkingTrace`** is a distinct channel from `message` — reasoning is streamed
  separately and collapsible in the UI.
- **Citations are split**: `webSearchResults` (all hits) vs `citedWebSearchResults`
  (actually referenced), and likewise `ragResults` vs `citedRagResults`.
- **`steps`** is an ordered agent trace; `toolUsageCard` appears 83× in the main
  client chunk, so tool activity is a first-class rendered card, not plain text.

## Conversation model

```
conversationId, title, createTime, modifyTime, starred,
systemPromptName, isXShared, controller
```

`starred` is server-side. `controller` indicates an inflight owner.

## Streaming resilience

```
/rest/app-chat/conversations/reconnect-response/{responseId}
/rest/app-chat/conversations/reconnect-response-v2/{responseId}
/rest/app-chat/conversations/inflight-response/{responseId}
/rest/app-chat/conversations/{id}/stop-inflight-responses
```

A dropped stream is *resumable by response id* rather than restarted. Any client
that wants parity must persist the response id before the first token arrives.

## Feature surface (grouped from 533 routes)

| Capability | Evidence |
| --- | --- |
| Chat + tree branching | `app-chat/conversations/*`, `response-node`, `choose_sbs_response` |
| Side-by-side model compare | `enableSideBySide`, `forceSideBySide`, `grok-4-fast-side-by-side` |
| Reasoning / Think | `isReasoning`, `thinkingTrace`, `grok-3-reasoning` |
| DeepSearch | `deepsearchPreset`, `deepSearchSupportsTrace`, `webSearchResults` |
| Image generation + edit | `image-generations`, `enableImageStreaming`, `imageEditUri(s)`, `generatedImageUrls` |
| Vision / files | `upload-file-v2/{init,patch,complete,abort,status}`, `file-metadata/{id}` |
| Resumable multipart upload | `{partNumber, etag}`, `{uploadId, fileMetadata}` |
| Voice mode | `voice/post-recording`, `voice/share`, `read-response-audio/{id}` |
| TTS | `text-to-speech`, `tts-voices` → `{voiceId,name,language,gender,age,previewUrl}` |
| Code execution | `run-code`, `agent-sandbox/sessions/*` |
| Agent sandbox lifecycle | `sessions/{id}/{hibernate,restore,snapshot,status,logs,preview}` |
| App building + deploy | `app-deployer/v1/projects/*`, `custom-domains`, `env-vars:batchSet` |
| Memory | `memory`, `memory_v2/{id}`, `memories_v2/{companionId}`, `import-memory`, `user-memory-blurb`, `disableMemory` |
| Connectors / RAG | `connectors/*` (Google Drive, OneDrive, Box, Plaid finance), `collectionIds`, `ragResults` |
| GitHub integration | `github/repositories/{owner}/{repo}/{branches,compare,pull-requests}` |
| Automations | `automations/{taskId}/{enabled,run,runs}`, `webhook/rotate-secret` |
| Sharing | `share_links/{id}/{clone,clone-sandbox,subagents}`, `ask-to-share` |
| Assets / versioning | `assets/{id}/{versions,clone,restore}`, `assets/storage-usage` |
| Suggestions | `conversation-starters`, `suggestions`, `fetch-suggestions`, `quick-answer` |
| Personas | `systemPromptName`, `customPersonality`, `customInstructions`, `template-prompt` |
| Ephemeral chat | `temporary` |
| Finance vertical | `finance/{ticker}/{summary,chart,financials,related_tickers}` |

## Error semantics

The client defines `RateLimitError` ("Too many requests. Try again later.") thrown on
HTTP 429, and a `StreamError` wrapper carrying a typed error. It also treats
`"Responses missing, errored, or not closed"` and `"First response is missing in
responses"` as a known recoverable set — i.e. the stream contract expects a terminal
close marker and the client detects its absence.

## What this project does with the findings

`grok-rs` is a **bring-your-own-endpoint** client. It does not talk to `grok.com`'s
cookie-authenticated private API, and it does not attempt to bypass xAI
authentication, rate limits, or paywalls. It speaks the public OpenAI-compatible
chat-completions protocol (which `api.x.ai/v1` itself implements) plus Anthropic
Messages, and reproduces the *interaction model* recovered above:

- response tree with branch/regenerate instead of a flat message list
- separate reasoning channel, collapsible
- split "all results" vs "cited results" for search and RAG
- tool activity as structured cards with an approval gate
- persisted response ids so a stream can be resumed
- server-side-equivalent starring, personas, temporary chat, memory

Fields that require xAI-proprietary backends (Imagine image streaming, voice
recording upload, the deploy sandbox) are implemented against provider-agnostic
equivalents and are marked as provider-dependent in the README rather than faked.