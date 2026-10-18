// Independent measurement harness for the workbench derived-value read path.
//
// It is bundled by `scripts/measure_workbench.mjs` (esbuild) and executed with
// plain Node: it imports the *real* production modules from `ui/src/lib` and
// records medians for the repeated derivations a single workbench render (and a
// single "生成" click) performs:
//
//   * data.isDirty             -> documentValue() -> encode(asset)
//   * documentExecution.documentSignature
//   * appDocumentParameters.documentForGeneration
//   * workbenchDerived.filterCatalogItems / catalogTags
//   * templateCatalog.loadTemplateCatalog fan-out (N+1 template.read)
//
// Nothing here starts the WebUI server, touches a user workspace or writes into
// version-controlled files; the caller redirects the JSON payload to the
// Git-ignored target/agent-preview/optimization tree.

import { performance } from "node:perf_hooks";
import process from "node:process";
import {
  documentValue,
  isDirty,
  newDocument,
  type Document,
} from "../ui/src/lib/data";
import { documentSignature } from "../ui/src/lib/documentExecution";
import { documentForGeneration } from "../ui/src/lib/appDocumentParameters";
import {
  catalogTags,
  filterCatalogItems,
  type CatalogItemLike,
} from "../ui/src/lib/workbenchDerived";
import { loadTemplateCatalog } from "../ui/src/lib/templateCatalog";

const PREFIX = "WORKBENCH_METRIC_JSON ";

function emit(payload: Record<string, unknown>): void {
  process.stdout.write(PREFIX + JSON.stringify(payload) + "\n");
}

function median(values: number[]): number {
  const sorted = [...values].sort((a, b) => a - b);
  const mid = Math.floor(sorted.length / 2);
  return sorted.length % 2
    ? sorted[mid]
    : (sorted[mid - 1] + sorted[mid]) / 2;
}

function timeReps(reps: number, run: () => void): number[] {
  const samples: number[] = [];
  for (let i = 0; i < reps; i += 1) {
    const started = performance.now();
    run();
    samples.push(performance.now() - started);
  }
  return samples;
}

function sourceOf(bytes: number, index: number): string {
  // A plausible Jinja body of roughly `bytes` characters.
  const line = `{% if item_${index} %}{{ value_${index} | default("x") }}{% endif %}\n`;
  return line.repeat(Math.ceil(bytes / line.length)).slice(0, bytes);
}

function contextOf(bytes: number, index: number): Record<string, unknown> {
  return {
    width: 12.5 + index,
    height: 4,
    label: "文档 " + index,
    notes: "n".repeat(Math.max(0, bytes - 64)),
  };
}

function makeDocument(sizeBytes: number, index: number): Document {
  const doc = newDocument();
  doc.id = "tpl-" + index;
  doc.title = "模板 " + index;
  doc.outputName = "tpl-" + index + ".txt";
  doc.asset = {
    source: sourceOf(sizeBytes, index),
    schema: {
      type: "object",
      properties: {
        width: { type: "number", title: "宽度" },
        height: { type: "integer", title: "高度" },
        label: { type: "string", title: "标签" },
        notes: { type: "string", title: "备注" },
      },
      required: ["width"],
    },
    defaults: { width: 10, height: 4 },
    tags: ["演示", "面板"],
    metadata: { parameters: { selection: {} }, title: "模板 " + index },
  };
  doc.context = contextOf(Math.max(256, Math.floor(sizeBytes / 10)), index);
  doc.jsonInput = JSON.stringify(doc.context);
  // Steady state: the draft matches what was saved, which is the common case in
  // the tab strip. `documentValue` is computed once so `isDirty` returns false.
  doc.saved = documentValue(doc);
  return doc;
}

function measureRecompute(counts: number[], sizes: number[], reps: number): void {
  for (const count of counts) {
    for (const size of sizes) {
      const docs = Array.from({ length: count }, (_, i) => makeDocument(size, i));
      const active = docs[docs.length - 1];

      const dirtySamples = timeReps(reps, () => {
        docs.forEach(isDirty);
      });
      const signSamples = timeReps(reps, () =>
        documentSignature(active, "template.render"),
      );
      const genSamples = timeReps(reps, () => documentForGeneration(active));

      // One workbench render pass for the active document: the tab strip marks
      // every open document dirty, then the toolbar + stale check derive the
      // active document's signature and generation context.
      const renderPass = timeReps(reps, () => {
        docs.forEach(isDirty);
        documentSignature(active, "template.render");
        try {
          documentForGeneration(active);
        } catch {
          /* same guard App keeps for inputError */
        }
      });
      // Same documents reference: App's useMemo reuses these dirty keys for
      // unrelated state updates. Keep signature/context work in both samples.
      const dirtyKeys = new Set(docs.filter(isDirty).map(doc => doc.key));
      const cachedRenderPass = timeReps(reps, () => {
        docs.forEach(doc => dirtyKeys.has(doc.key));
        documentSignature(active, "template.render");
        documentForGeneration(active);
      });
      // One "生成" click: documentForGeneration is derived twice (validation +
      // documentInput) and documentSignature three times (render, execution,
      // lastRunSignature) for the same unchanged document.
      const generatePath = timeReps(reps, () => {
        documentForGeneration(active);
        documentForGeneration(active);
        documentSignature(active, "template.render");
        documentSignature(active, "template.render");
        documentSignature(active, "template.render");
      });

      emit({
        section: "recompute",
        count,
        size,
        reps,
        dirty_all_ms: median(dirtySamples),
        dirty_one_ms: median(dirtySamples) / count,
        signature_one_ms: median(signSamples),
        for_generation_one_ms: median(genSamples),
        render_pass_ms: median(renderPass),
        cached_render_pass_ms: median(cachedRenderPass),
        generate_path_ms: median(generatePath),
      });
    }
  }
}

function makeItems(count: number): CatalogItemLike[] {
  const words = ["同步", "粗车", "面板", "演示", "参数", "轨迹", "汇总"];
  return Array.from({ length: count }, (_, i) => ({
    id: "asset-" + i,
    title: "模板 " + i,
    description: "生成的示例模板 " + i,
    tags: [words[i % words.length], words[(i * 3) % words.length]],
  }));
}

function measureFilter(counts: number[], reps: number): void {
  for (const count of counts) {
    const items = makeItems(count);
    const query = "模板 42";
    const tag = "演示";

    const pure = timeReps(reps, () => {
      filterCatalogItems(items, query, tag);
    });
    const tagsOnly = timeReps(reps, () => {
      catalogTags(items);
    });
    // The inline form App.tsx uses today, kept verbatim for an apples-to-apples
    // baseline against the extracted pure helper.
    const inline = timeReps(reps, () => {
      items.filter(
        (item) =>
          (!query ||
            [item.title, item.description, item.id, ...item.tags]
              .join(" ")
              .toLowerCase()
              .includes(query.toLowerCase())) &&
          (!tag || item.tags.includes(tag)),
      );
    });
    const inlineTags = timeReps(reps, () => {
      [...new Set(items.flatMap((item) => item.tags))];
    });

    emit({
      section: "filter",
      count,
      reps,
      pure_filter_ms: median(pure),
      inline_filter_ms: median(inline),
      pure_tags_ms: median(tagsOnly),
      inline_tags_ms: median(inlineTags),
    });
  }
}

interface CatalogMock {
  catalog: () => Promise<unknown>;
  read: (input: { name: string }) => Promise<unknown>;
}

function installMock(count: number, latencyMs: number): () => number {
  let current = 0;
  let peak = 0;
  const mock: CatalogMock = {
    catalog: async () => ({
      data: { items: Array.from({ length: count }, (_, i) => ({ id: "t-" + i })) },
      diagnostics: [],
    }),
    read: async (input) => {
      current += 1;
      if (current > peak) peak = current;
      await new Promise((resolve) => setTimeout(resolve, latencyMs));
      current -= 1;
      return {
        data: {
          asset: { source: "{{ x }}", metadata: { title: input.name } },
          fingerprint: "fp-" + input.name,
        },
      };
    },
  };
  (globalThis as { __WORKBENCH_ACTION__?: CatalogMock }).__WORKBENCH_ACTION__ =
    mock;
  return () => peak;
}

// Strategy comparison independent of which loadTemplateCatalog build is bundled:
// the same mocked read fanned out unbounded vs through a fixed-size pool.
async function measureStrategies(
  counts: number[],
  latencyMs: number,
  limit: number,
): Promise<void> {
  for (const count of counts) {
    const items = Array.from({ length: count }, (_, i) => ({ id: "t-" + i }));
    const read = async (): Promise<number> => {
      await new Promise((resolve) => setTimeout(resolve, latencyMs));
      return 1;
    };

    let unboundedPeak = 0;
    let unboundedCurrent = 0;
    const tracked = async (): Promise<number> => {
      unboundedCurrent += 1;
      if (unboundedCurrent > unboundedPeak) unboundedPeak = unboundedCurrent;
      try {
        return await read();
      } finally {
        unboundedCurrent -= 1;
      }
    };
    const unboundedStart = performance.now();
    await Promise.allSettled(items.map(tracked));
    const unboundedWall = performance.now() - unboundedStart;

    let boundedPeak = 0;
    let boundedCurrent = 0;
    let next = 0;
    const boundedTracked = async (): Promise<void> => {
      boundedCurrent += 1;
      if (boundedCurrent > boundedPeak) boundedPeak = boundedCurrent;
      try {
        await read();
      } finally {
        boundedCurrent -= 1;
      }
    };
    const worker = async (): Promise<void> => {
      while (next < items.length) {
        next += 1;
        await boundedTracked();
      }
    };
    const boundedStart = performance.now();
    await Promise.all(
      Array.from({ length: Math.min(limit, items.length) }, worker),
    );
    const boundedWall = performance.now() - boundedStart;

    emit({
      section: "catalog-strategy",
      count,
      latency_ms: latencyMs,
      limit,
      unbounded_wall_ms: unboundedWall,
      unbounded_peak: unboundedPeak,
      bounded_wall_ms: boundedWall,
      bounded_peak: boundedPeak,
    });
  }
}

async function measureCatalog(counts: number[], latencyMs: number): Promise<void> {
  for (const count of counts) {
    const peak = installMock(count, latencyMs);
    const started = performance.now();
    const result = await loadTemplateCatalog();
    const wallMs = performance.now() - started;
    const first = result.items[0];
    emit({
      section: "catalog",
      count,
      latency_ms: latencyMs,
      wall_ms: wallMs,
      peak_in_flight: peak(),
      items: result.items.length,
      order_preserved: first?.id === "t-0",
      fingerprint_preserved: first?.fingerprint === "fp-t-0",
      diagnostics: result.diagnostics.length,
    });
  }
}

async function main(): Promise<void> {
  const counts = (process.env.WORKBENCH_COUNTS || "10,100")
    .split(",")
    .map(Number);
  const sizes = (process.env.WORKBENCH_SIZES || "10240,102400")
    .split(",")
    .map(Number);
  const reps = Number(process.env.WORKBENCH_ITERATIONS || "30");
  const limit = Number(process.env.WORKBENCH_CATALOG_LIMIT || "8");
  const sections = (
    process.env.WORKBENCH_SECTIONS || "recompute,filter,catalog"
  )
    .split(",")
    .map((s) => s.trim())
    .filter(Boolean);

  if (sections.includes("recompute")) measureRecompute(counts, sizes, reps);
  if (sections.includes("filter")) measureFilter([100, 1000], Math.max(reps, 50));
  if (sections.includes("catalog-strategy"))
    await measureStrategies(counts, 2, limit);
  if (sections.includes("catalog")) await measureCatalog(counts, 2);
}

void main();
