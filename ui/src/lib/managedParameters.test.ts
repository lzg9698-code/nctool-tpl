import { describe, expect, it } from "vitest";
import { managedRawInput, managedFormSchema, validatedSelection } from "./managedParameters";
import { documentInput, documentSignature } from "./documentExecution";
import { encode, newDocument } from "./data";
describe("managed original inputs", () => {
  it("blocks empty and invalid machining JSON drafts instead of using previous values", () => {
    expect(() => validatedSelection({slots:{rough:{overrides:{rpm:1000},overrides_draft:""}}})).toThrow("JSON 草稿");
    expect(() => validatedSelection({slots:{rough:{inputs_draft:"{"}}})).toThrow("JSON 草稿");
    expect(validatedSelection({slots:{rough:{overrides:{rpm:1000},overrides_draft:undefined}}}).slots.rough.overrides.rpm).toBe(1000);
  });
  it("omits selected tool facts while preserving dimensions and untouched slots", () => {
    const value = { geometry: { depth: 13.1 }, rough_tool: { diameter: 0, cutting_speed: 100 }, finish_tool: { diameter: 8 } };
    const bindings = { "/rough_tool/diameter": { slot: "rough" }, "/finish_tool/diameter": { slot: "finish" } };
    expect(JSON.parse(encode(managedRawInput(value, bindings, { slots: { rough: { tool_id: "T1" } } })))).toEqual({ geometry: { depth: 13.1 }, rough_tool: { cutting_speed: 100 }, finish_tool: { diameter: 8 } });
    expect(value.rough_tool.diameter).toBe(0);
    const schema = { type: "object", properties: { rough_tool: { type: "object", properties: { diameter: { type: "number" } } } } };
    expect(managedFormSchema(schema, bindings, { slots: { rough: { tool_id: "T1" } } }).properties.rough_tool.properties.diameter.readOnly).toBe(true);
    expect(schema.properties.rough_tool.properties.diameter).not.toHaveProperty("readOnly");
  });
  it("includes transient selection and confirmation in execution dependencies", () => {
    const doc = newDocument(); doc.context = { raw: 10 };
    const entry = { id: "nc.generate.managed", ui_schema: { document_input: { bindings: { operation_inputs: "context", parameter_selection: "parameter_selection" } } } };
    const original = documentSignature(doc, entry.id);
    doc.parameterSelection = { slots: { rough: { tool_id: "T1", confirmed_for: "a" } } };
    expect(documentInput(doc, entry).parameter_selection).toEqual(doc.parameterSelection);
    expect(documentSignature(doc, entry.id)).not.toBe(original);
    const signature = documentSignature(doc, entry.id); doc.parameterSelection.slots.rough.confirmed_for = "b";
    expect(documentSignature(doc, entry.id)).not.toBe(signature);
  });
});
