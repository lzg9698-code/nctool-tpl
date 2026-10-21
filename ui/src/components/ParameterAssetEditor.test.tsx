// @vitest-environment jsdom
import { useState, type ReactElement } from "react";
import { afterEach, describe, expect, it } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { DefinitionEditor, PresetEditor } from "./ParameterAssetEditor";
import { encode, numberText, parseData, type Data } from "../lib/data";

/** Numeric values are LosslessNumber tokens; compare their exact literal text. */
function exact(value: Data): string {
  return numberText(value);
}

// Render the real editor over real state so the assertions describe what an
// author actually gets, not a stubbed surface.
function Harness({ asset, children }: { asset: Data; children: (asset: Data, patch: (patch: Data) => void) => ReactElement }) {
  const [value, setValue] = useState<Data>(asset);
  const patch = (next: Data) => {
    const merged = { ...value, ...next };
    for (const key of Object.keys(next)) if (next[key] === undefined) delete merged[key];
    setValue(merged);
  };
  return (
    <>
      {children(value, patch)}
      <output aria-label="资产快照">{encode(value)}</output>
    </>
  );
}
function snapshot(): Data {
  return parseData(screen.getByLabelText("资产快照").textContent!);
}

afterEach(cleanup);

describe("DefinitionEditor", () => {
  it("writes plain definition fields without inventing a schema unit", () => {
    const asset = { format_version: 1, id: "depth", title: "深度", schema: { type: "number" }, unit: "mm", group: "零件", description: "目标深度" };
    render(
      <Harness asset={asset}>
        {(value, patch) => <DefinitionEditor asset={value} stored={false} onChange={patch} />}
      </Harness>,
    );
    fireEvent.change(screen.getByLabelText("显示名称"), { target: { value: "加工深度" } });
    fireEvent.change(screen.getByLabelText("单位"), { target: { value: "cm" } });
    fireEvent.change(screen.getByLabelText("分组"), { target: { value: "车削" } });
    expect(snapshot()).toMatchObject({ title: "加工深度", unit: "cm", group: "车削" });
    // The asset-level unit must not leak into the schema as x-unit.
    expect(snapshot().schema).toEqual({ type: "number" });
  });

  it("switches type and drops containers that no longer apply", () => {
    const asset = { format_version: 1, id: "d", title: "d", schema: { type: "object", properties: { a: { type: "string" } }, required: ["a"] } };
    render(
      <Harness asset={asset}>
        {(value, patch) => <DefinitionEditor asset={value} stored onChange={patch} />}
      </Harness>,
    );
    fireEvent.change(screen.getByLabelText("参数类型"), { target: { value: "string" } });
    expect(snapshot().schema).toEqual({ type: "string" });
  });

  it("marks the identifier read-only for a stored definition", () => {
    const asset = { format_version: 1, id: "depth", title: "深度", schema: { type: "string" } };
    render(
      <Harness asset={asset}>
        {(value, patch) => <DefinitionEditor asset={value} stored onChange={patch} />}
      </Harness>,
    );
    expect((screen.getByLabelText("资产标识") as HTMLInputElement).readOnly).toBe(true);
    expect(screen.getByText(/标识即文件名/)).toBeTruthy();
  });

  it("removes the default entirely rather than storing null when unchecked", () => {
    const asset = { format_version: 1, id: "d", title: "d", schema: { type: "string" }, default: "x" };
    render(
      <Harness asset={asset}>
        {(value, patch) => <DefinitionEditor asset={value} stored onChange={patch} />}
      </Harness>,
    );
    fireEvent.click(screen.getByLabelText("提供默认值"));
    expect(snapshot()).not.toHaveProperty("default");
  });

  it("edits a numeric default through the definition schema", () => {
    const asset = { format_version: 1, id: "d", title: "d", schema: { type: "number" }, default: 3 };
    render(
      <Harness asset={asset}>
        {(value, patch) => <DefinitionEditor asset={value} stored onChange={patch} />}
      </Harness>,
    );
    fireEvent.change(screen.getByLabelText("值"), { target: { value: "12.5" } });
    expect(exact(snapshot().default)).toBe("12.5");
  });
});

describe("PresetEditor", () => {
  const definitions = [
    { id: "depth", title: "深度", asset: { schema: { type: "number" } } },
    { id: "label", title: "标签", asset: { schema: { type: "string" } } },
  ];

  it("changes only the targeted definition and keeps exact numeric literals", () => {
    const asset = { format_version: 1, id: "rough", title: "粗加工", values: { depth: 5 } };
    render(
      <Harness asset={asset}>
        {(value, patch) => <PresetEditor asset={value} stored definitions={definitions} onChange={patch} />}
      </Harness>,
    );
    // Beyond Number.MAX_SAFE_INTEGER: the literal must survive, not round to 9007199254740992.
    fireEvent.change(screen.getByLabelText("取值 depth"), { target: { value: "9007199254740993" } });
    expect(exact(snapshot().values.depth)).toBe("9007199254740993");
    expect(screen.getByLabelText("取值 label")).toBeTruthy();
  });

  it("keeps a stored value whose definition no longer exists instead of dropping it", () => {
    const asset = { format_version: 1, id: "p", title: "p", values: { gone: 7 } };
    render(
      <Harness asset={asset}>
        {(value, patch) => <PresetEditor asset={value} stored definitions={definitions} onChange={patch} />}
      </Harness>,
    );
    expect(screen.getByText(/该定义当前不存在/)).toBeTruthy();
    fireEvent.change(screen.getByLabelText("取值 depth"), { target: { value: "1" } });
    const values = snapshot().values;
    expect(Object.keys(values).sort()).toEqual(["depth", "gone"]);
    expect(exact(values.gone)).toBe("7");
    expect(exact(values.depth)).toBe("1");
  });

  it("clears a value without removing the row", () => {
    const asset = { format_version: 1, id: "p", title: "p", values: { depth: 5 } };
    render(
      <Harness asset={asset}>
        {(value, patch) => <PresetEditor asset={value} stored definitions={definitions} onChange={patch} />}
      </Harness>,
    );
    fireEvent.click(screen.getByLabelText("清除取值 depth"));
    expect(snapshot().values).not.toHaveProperty("depth");
    expect(screen.getByLabelText("取值 depth")).toBeTruthy();
  });

  it("does not clobber an author value when adding a row with the same id", () => {
    const asset = { format_version: 1, id: "p", title: "p", values: { depth: 9 } };
    render(
      <Harness asset={asset}>
        {(value, patch) => <PresetEditor asset={value} stored definitions={definitions} onChange={patch} />}
      </Harness>,
    );
    fireEvent.change(screen.getByLabelText("新增覆盖定义标识"), { target: { value: "depth" } });
    fireEvent.click(screen.getByRole("button", { name: "添加覆盖" }));
    expect(exact(snapshot().values.depth)).toBe("9");
  });

  it("adds an unknown definition row that stays visible", () => {
    const asset = { format_version: 1, id: "p", title: "p", values: {} };
    render(
      <Harness asset={asset}>
        {(value, patch) => <PresetEditor asset={value} stored definitions={definitions} onChange={patch} />}
      </Harness>,
    );
    fireEvent.change(screen.getByLabelText("新增覆盖定义标识"), { target: { value: "future" } });
    fireEvent.click(screen.getByRole("button", { name: "添加覆盖" }));
    expect(snapshot().values).toHaveProperty("future");
    expect(screen.getByLabelText("取值 future")).toBeTruthy();
  });
});
