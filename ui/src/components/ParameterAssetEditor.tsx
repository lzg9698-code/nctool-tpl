import { useState } from "react";
import { Plus, Trash2 } from "lucide-react";
import {
  encode,
  numberText,
  typeOf,
  DraftNumber,
  type Data,
} from "../lib/data";
import { Form } from "./Form";
import { catalogLabel } from "../lib/managedParameters";

// Structured editors for the two shared-parameter asset kinds. Both write the
// same asset object as the raw JSON editor, so the form is a readable view over
// the asset rather than a second source of truth.
//
// A definition separates two ideas that are easy to confuse:
//   - asset-level `unit`: the physical unit of this value (for example mm),
//     matched against a binding's `unit` by the parameters plugin.
//   - schema-level `x-unit`: a template field's own unit hint, which belongs to
//     the template schema editor, not here.
const DEFINITION_TYPES = [
  ["string", "文本"],
  ["number", "数值"],
  ["integer", "整数"],
  ["boolean", "开关"],
  ["object", "对象 / 分组"],
  ["array", "数组 / 列表"],
];

function assetTitle(asset: Data): string {
  return typeof asset.title === "string" ? asset.title : "";
}

/** Shared identity fields: `id` is the file name, so it is stable and read-only once saved. */
function IdentityFields({
  asset,
  stored,
  onChange,
}: {
  asset: Data;
  stored: boolean;
  onChange: (patch: Data) => void;
}) {
  return (
    <div className="two-fields">
      <label>
        显示名称
        <input
          aria-label="显示名称"
          value={assetTitle(asset)}
          onChange={(event) => onChange({ title: event.target.value })}
          placeholder="例如 加工深度"
        />
      </label>
      <label>
        标识
        <input
          aria-label="资产标识"
          value={asset.id || ""}
          readOnly={stored}
          onChange={(event) =>
            stored ? undefined : onChange({ id: event.target.value })
          }
          placeholder="例如 part-depth"
        />
        {stored && <span className="parameter-hint">标识即文件名，保存后保持稳定，不随显示名称变化。</span>}
      </label>
    </div>
  );
}

export function DefinitionEditor({
  asset,
  stored,
  onChange,
}: {
  asset: Data;
  stored: boolean;
  onChange: (patch: Data) => void;
}) {
  const schema: Data = asset.schema && typeof asset.schema === "object" ? asset.schema : { type: "string" };
  const type = typeOf({ ...schema, "x-untyped": undefined });
  const typed = DEFINITION_TYPES.some(([key]) => key === schema.type) || Array.isArray(schema.type);
  function patchSchema(patch: Data) {
    onChange({ schema: { ...schema, ...patch } });
  }
  function setType(next: string) {
    const updated: Data = { ...schema, type: next };
    delete updated["x-untyped"];
    if (next !== "object") {
      delete updated.properties;
      // `required` only means anything for an object; leaving it on a scalar
      // schema would describe a constraint the type cannot have.
      delete updated.required;
    }
    if (next !== "array") delete updated.items;
    delete updated.enum;
    onChange({ schema: updated });
  }
  return (
    <div className="parameter-asset-editor">
      <IdentityFields asset={asset} stored={stored} onChange={onChange} />
      <div className="two-fields">
        <label>
          参数类型
          <select
            aria-label="参数类型"
            value={typed ? type : ""}
            onChange={(event) => setType(event.target.value)}
          >
            <option value="" disabled>
              请选择类型
            </option>
            {DEFINITION_TYPES.map(([key, label]) => (
              <option key={key} value={key}>
                {label}
              </option>
            ))}
          </select>
        </label>
        <label>
          单位
          <input
            aria-label="单位"
            value={asset.unit || ""}
            onChange={(event) => onChange({ unit: event.target.value })}
            placeholder="例如 mm，可留空"
          />
        </label>
      </div>
      <div className="two-fields">
        <label>
          分组
          <input
            aria-label="分组"
            value={asset.group || ""}
            onChange={(event) => onChange({ group: event.target.value })}
            placeholder="用于在表单里归类，可留空"
          />
        </label>
        <label>
          说明
          <input
            aria-label="说明"
            value={asset.description || ""}
            onChange={(event) => onChange({ description: event.target.value })}
            placeholder="说明这个参数的用途，可留空"
          />
        </label>
      </div>
      {(type === "number" || type === "integer") && (
        <div className="three-fields">
          <label>
            最小值
            <input
              aria-label="最小值"
              value={numberText(schema.minimum)}
              inputMode="decimal"
              onChange={(event) =>
                patchSchema({
                  minimum: event.target.value ? new DraftNumber(event.target.value) : undefined,
                })
              }
            />
          </label>
          <label>
            最大值
            <input
              aria-label="最大值"
              value={numberText(schema.maximum)}
              inputMode="decimal"
              onChange={(event) =>
                patchSchema({
                  maximum: event.target.value ? new DraftNumber(event.target.value) : undefined,
                })
              }
            />
          </label>
          <label className="checkbox">
            <input
              type="checkbox"
              aria-label="必须大于最小值"
              checked={schema.exclusiveMinimum !== undefined}
              onChange={(event) =>
                patchSchema({
                  exclusiveMinimum: event.target.checked ? schema.minimum ?? new DraftNumber("0") : undefined,
                })
              }
            />
            不包含最小值
          </label>
        </div>
      )}
      {type === "string" && (
        <label>
          候选值（每行一个，留空则自由填写）
          <textarea
            aria-label="候选值"
            rows={3}
            value={(schema.enum || []).join("\n")}
            onChange={(event) =>
              patchSchema({
                enum: event.target.value ? event.target.value.split("\n").filter(Boolean) : undefined,
              })
            }
          />
        </label>
      )}
      {(type === "object" || type === "array") && (
        <p className="parameter-hint">
          嵌套结构的类型定义请使用下方“高级 JSON”，或在模板的“参数定义”中编辑。
        </p>
      )}
      <DefinitionDefault asset={asset} schema={schema} onChange={onChange} />
    </div>
  );
}

/** The optional default is edited against the definition's own schema. */
function DefinitionDefault({
  asset,
  schema,
  onChange,
}: {
  asset: Data;
  schema: Data;
  onChange: (patch: Data) => void;
}) {
  // Derived from the asset rather than local state: the editor stays mounted
  // across asset switches, so a local copy would disagree with the draft.
  const enabled = Object.hasOwn(asset, "default");
  let rendered = "";
  try {
    rendered = enabled ? encode(asset.default) : "";
  } catch {
    rendered = "";
  }
  return (
    <div className="definition-default">
      <label className="checkbox">
        <input
          type="checkbox"
          aria-label="提供默认值"
          checked={enabled}
          onChange={(event) => {
            // Removing the field entirely keeps "no default" distinct from a
            // stored null, which is a valid value for a nullable schema.
            if (!event.target.checked) onChange({ default: undefined });
            else onChange({ default: typeOf(schema) === "object" ? {} : "" });
          }}
        />
        提供默认值
      </label>
      {enabled && (
        <>
          <Form schema={schema} value={asset.default} onChange={(value) => onChange({ default: value })} />
          <p className="parameter-hint">当前默认值：{rendered || "（空）"}。默认值必须符合上面的类型与范围。</p>
        </>
      )}
    </div>
  );
}

export interface CatalogEntry {
  id: string;
  title?: string;
  asset?: Data;
}

// Values are typed by the definition they belong to: numeric definitions keep a
// DraftNumber so exact literals survive, everything else is plain text.
function definitionType(entry?: CatalogEntry): string {
  const schema = entry?.asset?.schema;
  return schema && typeof schema === "object" ? typeOf(schema) : "string";
}

export function PresetEditor({
  asset,
  stored,
  definitions,
  onChange,
}: {
  asset: Data;
  stored: boolean;
  definitions: CatalogEntry[];
  onChange: (patch: Data) => void;
}) {
  const values: Data = asset.values && typeof asset.values === "object" ? asset.values : {};
  const [newId, setNewId] = useState("");
  const known = new Map(definitions.map((item) => [item.id, item]));
  // Keep an explicitly stored key even when its definition is gone: dropping it
  // silently would discard authored data on an unrelated edit.
  const rows = [...new Set([...Object.keys(values), ...definitions.map((item) => item.id)])].sort();
  function setValue(id: string, raw: string) {
    const next = { ...values };
    if (raw === "") delete next[id];
    else if (["number", "integer"].includes(definitionType(known.get(id))))
      next[id] = new DraftNumber(raw);
    else next[id] = raw;
    onChange({ values: next });
  }
  function addUnknown() {
    const id = newId.trim();
    // Adding a row must not clobber a value the author already stored.
    if (!id || Object.hasOwn(values, id)) return;
    onChange({ values: { ...values, [id]: "" } });
    setNewId("");
  }
  return (
    <div className="parameter-asset-editor">
      <IdentityFields asset={asset} stored={stored} onChange={onChange} />
      <div className="preset-values">
        <strong>取值</strong>
        <p className="parameter-hint">
          共享预置按定义标识提供数值，同一预置可覆盖多个定义。未填写的定义不会被覆盖。
        </p>
        {rows.map((id) => {
          const definition = known.get(id);
          const present = Object.hasOwn(values, id);
          return (
            <div className="preset-row" key={id}>
              <span>
                {definition ? catalogLabel({ id, title: definition.title ?? definition.id }) : id}
                {!definition && <em className="parameter-hint">（该定义当前不存在，保留已填写值）</em>}
              </span>
              <input
                aria-label={`取值 ${id}`}
                value={present ? numberText(values[id]) : ""}
                onChange={(event) => setValue(id, event.target.value)}
                placeholder={present ? "" : "未覆盖"}
              />
              {present && (
                <button
                  type="button"
                  className="text-button"
                  aria-label={`清除取值 ${id}`}
                  onClick={() => setValue(id, "")}
                >
                  <Trash2 size={12} />
                  清除
                </button>
              )}
            </div>
          );
        })}
        <div className="new-field">
          <input
            aria-label="新增覆盖定义标识"
            value={newId}
            onChange={(event) => setNewId(event.target.value)}
            placeholder="定义标识，例如 part-depth"
            onKeyDown={(event) => {
              if (event.key === "Enter") {
                event.preventDefault();
                addUnknown();
              }
            }}
          />
          <button type="button" className="button secondary" onClick={addUnknown}>
            <Plus size={14} />
            添加覆盖
          </button>
        </div>
      </div>
    </div>
  );
}
