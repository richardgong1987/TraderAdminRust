# AI Training and Testing

Teach Gemma to answer the way you would (for example, BUY / SELL / HOLD on a chart), and measure whether
each round of training actually made it better.

## Problem

The user wants two pages:

1. **Training.** Paste images (chart screenshots) and text, and record the **standard answer** for each.
2. **Testing.** Check whether the trained model does better than before. If it doesn't, train more and
   test again.

Constraints:

- The server has no GPU, so Gemma's weights can't be fine-tuned there ([server-specs.md](../deploy/server-specs.md)).
- Gemma runs on a CPU. One answer takes seconds to minutes, so a test can't run inside a single HTTP request.
- "Did it improve?" has to be a number you can trust. A model is not tested on the same examples it learned
  from, or the score just measures memory.

## Approach

### Concepts

| Concept | What it is |
|---|---|
| **Example** | Images + text (the question), plus the standard answer: a **choice** (e.g. `BUY`) and an optional **explanation**. Each example is either **training** or **test**. |
| **Training set** | All training examples. The model learns from these. |
| **Test set** | All test examples. Kept apart: **the model never sees them while learning**, so the test score is honest. |
| **Model version** | The result of "Train". A version is Gemma + your **instructions** + the training examples it includes. Creating a version takes a second; nothing is computed until it's tested. |
| **Test run** | One version answering every test example. Each answer is scored, and the run's score is the share of correct choices. |

The loop the pages support:

```
add training examples ──► Train (new version) ──► Test run ──► compare with the previous version
        ▲                                                            │
        └──────────────── not better: add/fix examples or instructions ◄┘
```

### What "training" means in this version: prompt training

A version teaches by example. When it answers a question, the request to Gemma contains:

1. **System message:** the fixed answer rules (reply with JSON, one of the allowed choices), followed by
   the version's own instructions.
2. **One worked example per training example in the version:** a user message (its text + images),
   followed by an assistant message holding its standard answer.
3. **The question:** the test example's text + images.

Gemma copies the pattern of the examples. That works today on the CPU server, at no cost.
**Real fine-tuning can be added later** as a second kind of version, trained on a rented GPU from the
same training set. The pages, the data and the test runs stay the same.

### Scoring

- The allowed choices are all distinct choices in the current examples (e.g. `BUY`, `SELL`, `HOLD`). Adding
  an example with a new choice adds it.
- Ollama is called with a **JSON schema** (`format`), so Gemma must answer
  `{"choice": <one of the allowed choices>, "explanation": "..."}`. That means no free-text parsing.
- `temperature: 0`, so the same question gets the same answer, and score changes come from training, not chance.
- A result is **CORRECT** if the choice equals the expected choice (ignoring case and surrounding spaces),
  **WRONG** if it differs, and **ERROR** if Gemma or Ollama failed. The explanation is shown next to yours
  for you to read, but it is not scored.
- Run score = CORRECT ÷ total.

### Comparing runs

The test set changes over time, so two runs may not have answered the same questions. **Compare** only
counts the test examples that **both** runs answered, and lists each one: correct in A only, in B only, in both,
or in neither. That shows exactly what training fixed and what it broke.

### Running tests in the background

- **Start test** creates a run with status `RUNNING` and returns at once. A background task answers the
  test examples one after another and saves each result as it goes.
- The test page polls the run every few seconds and shows progress.
- **Only one run at a time.** Gemma uses all CPU cores, so parallel runs would only slow each other down.
- If the app restarts during a run, the run is marked `FAILED` ("interrupted") at startup instead of
  staying `RUNNING` forever.

### Where images are stored

In MySQL, base64-encoded, in their own table. Ollama takes images as base64 anyway.

- They are only served through logged-in API calls. The public `/admin/download` endpoint isn't used, so
  training data stays private.
- Database backups include them.
- Deleting an example deletes its images (foreign key cascade), so no files are left behind.
- Lists never load image data, only the image count. Images load when you open one example.

The browser scales pasted images down to at most 1600 px on the longest side before saving. Gemma's vision
encoder works at a lower resolution anyway, and this keeps rows and requests small.

### Rejected alternatives

| Alternative | Why not |
|---|---|
| Fine-tuning now | Needs a rented GPU and hours per run. Decided against for the first version. Prompt training gives a working loop today, and the data is reusable for fine-tuning later. |
| Test on the training examples | The score would measure memory, not learning. |
| Free-text answers judged by Gemma | Less reliable, and twice the CPU time per answer. The choice is scored exactly; the explanation is for reading. |
| Images as files through `/admin/upload` | That endpoint's downloads are public (needed for `<img>` elsewhere), pasted images are all named `image.png` and would overwrite each other, and files outlive deleted examples. |
| A frozen copy of each example inside each version | Images would be copied into every version. Versions reference examples instead, so **editing an example changes every version that uses it.** Test results keep a snapshot of the expected choice, so old scores stay valid. |

## Data

Six new tables, created on startup by the same table sync as the existing ones. Indexes and foreign keys
are added right after the sync. All IDs are `VARCHAR(50)` ObjectIds, like the rest of the app.

```
ai_example 1───* ai_example_image
     │
     *  (via ai_model_version_example)
     │
ai_model_version 1───* ai_eval_run 1───* ai_eval_result *───1 ai_example
```

| Table | Columns | Notes |
|---|---|---|
| `ai_example` | `id`, `purpose` (`TRAIN`/`TEST`), `question_text`, `expected_choice`, `expected_explanation` (NULL = none given), `create_by`, `create_date`, `update_date` | `INDEX(purpose, create_date)`: lists by purpose, newest first |
| `ai_example_image` | `id`, `example_id`, `sort_no`, `mime_type`, `data_base64` (LONGTEXT), `create_date` | FK → `ai_example` ON DELETE CASCADE. `INDEX(example_id, sort_no)`: load one example's images in order |
| `ai_model_version` | `id`, `name`, `base_model` (e.g. `gemma4:26b`), `instructions`, `create_by`, `create_date` | |
| `ai_model_version_example` | `id`, `version_id`, `example_id` | Junction table. `UNIQUE(version_id, example_id)`. FKs → both, ON DELETE CASCADE |
| `ai_eval_run` | `id`, `version_id`, `status` (`RUNNING`/`DONE`/`FAILED`), `total`, `error_message`, `create_by`, `create_date`, `finish_date` | FK → version, ON DELETE CASCADE. `total` is a snapshot of the test set size when the run started. |
| `ai_eval_result` | `id`, `run_id`, `example_id`, `expected_choice_snapshot`, `outcome` (`CORRECT`/`WRONG`/`ERROR`), `model_choice`, `model_explanation`, `error_message`, `prompt_tokens`, `duration_ms`, `create_date` | FK → run, ON DELETE CASCADE. `UNIQUE(run_id, example_id)`: one answer per example per run. `example_id` has no FK, so deleting a test example keeps old results. |

Not stored, but computed when read: a run's progress, correct count and score (from `ai_eval_result`), and
a version's example count.

`model_choice` / `model_explanation` are NULL exactly when `outcome = ERROR`, and `error_message` is set
exactly then. `outcome` is the single status.

## API

All endpoints require a login and follow the existing style: `POST` with JSON, `RespVO` responses.

| Endpoint | Does |
|---|---|
| `/admin/ai_example_page` | List examples by purpose, without image data |
| `/admin/ai_example_detail` | One example, with images |
| `/admin/ai_example_add`, `/admin/ai_example_update`, `/admin/ai_example_remove` | Edit examples. Update replaces the images. |
| `/admin/ai_choices` | Allowed choices (distinct `expected_choice`) |
| `/admin/ai_version_list`, `/admin/ai_version_add`, `/admin/ai_version_remove` | Versions. `add` is "Train". |
| `/admin/ai_eval_start` | Start a test run for a version |
| `/admin/ai_eval_list`, `/admin/ai_eval_detail`, `/admin/ai_eval_compare` | The latest 100 runs with scores, one run's answers, two runs side by side |

Config in `application.json5`:

| Key | Default | Environment override | Meaning |
|---|---|---|---|
| `ollama_url` | `http://127.0.0.1:11434` | `OLLAMA_URL` | Where Ollama is |
| `ollama_model` | `gemma4:26b` | `OLLAMA_MODEL` | Model for **new** versions; each version keeps the model it was created with |
| `ollama_num_ctx` | `8192` | | Context window sent with every request |

For local development on a Mac without the server, install Ollama, run `ollama pull gemma4:e2b`, and start the
app with `OLLAMA_MODEL=gemma4:e2b`.

## UI

New menu **AI**, with two pages in `abs_admin_vue`:

- **Training** (`/ai/train`): training examples, and a **Train** button. It opens a dialog to name the
  version, write instructions, and pick which examples to include: all, the selected rows, or none (a
  **baseline**, which is plain Gemma, the starting point to beat). Below it, the list of versions.
- **Testing** (`/ai/test`): test examples, **Start test** for a chosen version, the run history with scores
  and the change from the previous run, run details, and **Compare** for two runs.

Both pages share one example editor:
- Paste images with Ctrl/⌘+V, or drag them in, and see thumbnails.
- Type or paste the text.
- Pick or type the choice (suggestions come from existing choices).
- Add an optional explanation.

## Verified (2026-10-20)

Tested against MySQL 9.7.2 and real Gemma (`gemma4:e2b` locally; the server's `gemma4:26b` was still downloading):

- **Schema:** primary keys, `LONGTEXT` images, indexes and the five cascading foreign keys are created as
  described above. A second start adds nothing and logs no warnings.
- **API:** every endpoint, plus the rules:
  - validation (empty example, missing choice, more than 5 images, non-images);
  - test examples are refused as training data;
  - only one run at a time;
  - newest run first;
  - deleted test examples keep their results;
  - deleting a version deletes its runs.
- **Real answers:** Gemma answers with valid schema JSON, and images reach it. The second question of a
  run took 60–93 ms instead of 2–6 s, because Ollama reused the shared prompt prefix. `prompt_tokens` is the
  full prompt size, so the context warning works.
- **Restart:** a run left `RUNNING` is marked `FAILED` ("interrupted") on startup.
- **UI** (headless Chrome, Chinese locale):
  - paste an image;
  - fill in and save an example;
  - train a version;
  - test the version and a baseline;
  - compare the two runs;
  - open run details.

  No browser console errors.

## Risks

| Risk | Mitigation |
|---|---|
| Too many examples or images overflow Gemma's context window, and Ollama silently cuts the start of the prompt | Each result records `prompt_tokens`. The run details warn when it gets close to the context size. Images cost the most, so keep versions to a handful of the best examples. |
| Slow on CPU: each answer re-reads every example | Ollama reuses the cached prompt prefix when only the final question changes. The worked examples come first for exactly this reason. Real speed will be measured on the server. |
| Few-shot learning hits a ceiling | Expected. The training and test sets built here are exactly what fine-tuning needs later. |
| Editing an example changes old versions | Shown in the UI. Scores stay valid through the snapshots. |
| The Docker app can't reach Ollama, which listens on the host's `127.0.0.1` | Solved when the app is deployed to the server ([deploy/README.md](../deploy/README.md), Risks). Locally, use an SSH tunnel to the server's Ollama, or a local Ollama. |
| MySQL-specific column types (`LONGTEXT`) | The app is MySQL-only now ([deploy/README.md](../deploy/README.md)). |
