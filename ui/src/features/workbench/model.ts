import type { ReactNode } from "react";
import {
  defaultValue,
  documentValue,
  mergeDefaults,
  newDocument,
  type Asset,
  type Document,
  type Schema,
} from "../../lib/data";

export interface Item {
  id: string;
  title: string;
  description: string;
  tags: string[];
  fingerprint: string;
}

export interface Dialog {
  title: string;
  body: ReactNode;
  buttons?: ReactNode;
}

export const stateNames: Record<string, string> = {
  queued: "等待执行",
  running: "执行中",
  succeeded: "已完成",
  failed: "生成失败",
  cancelled: "已取消",
  interrupted: "已中断",
};

export function normalizeSchema(asset: Asset): Schema {
  return asset.schema && typeof asset.schema === "object"
    ? asset.schema
    : { type: "object", properties: {}, required: [] };
}

export function prepareDocument(
  id: string,
  asset: Asset,
  fingerprint: string,
): Document {
  const doc = newDocument();
  doc.id = id;
  doc.title = asset.metadata?.title || id;
  doc.asset = asset;
  doc.context = asset.metadata?.parameters
    ? // Shared parameters hold their own defaults, presets and definitions; the
      // context carries only this run's explicit overrides, so an inherited
      // value is never silently frozen into a manual one.
      {}
    : mergeDefaults(defaultValue(normalizeSchema(asset)), asset.defaults);
  doc.fingerprint = fingerprint;
  doc.mode = "use";
  doc.outputName = asset.metadata?.output_name || id + ".txt";
  doc.saved = documentValue(doc);
  return doc;
}
