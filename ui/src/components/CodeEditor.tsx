import { useEffect, useRef } from "react";
import { EditorView, keymap } from "@codemirror/view";
import { EditorState, Transaction } from "@codemirror/state";
import { basicSetup } from "codemirror";
import { indentWithTab } from "@codemirror/commands";
import { StreamLanguage } from "@codemirror/language";
import { jinja2 } from "@codemirror/legacy-modes/mode/jinja2";
import { json } from "@codemirror/lang-json";

export function CodeEditor({
  value,
  onChange,
  language = "jinja",
  label = "模板源码",
  readOnly = false,
  line,
}: {
  value: string;
  onChange?: (value: string) => void;
  language?: "jinja" | "json" | "text";
  label?: string;
  readOnly?: boolean;
  line?: number;
}) {
  const host = useRef<HTMLDivElement>(null),
    view = useRef<EditorView | null>(null),
    change = useRef(onChange);
  change.current = onChange;
  useEffect(() => {
    const editor = new EditorView({
      parent: host.current!,
      state: EditorState.create({
        doc: value,
        extensions: [
          basicSetup,
          keymap.of([indentWithTab]),
          language === "json"
            ? json()
            : language === "jinja"
              ? StreamLanguage.define(jinja2)
              : [],
          EditorState.readOnly.of(readOnly),
          EditorView.editable.of(!readOnly),
          EditorView.contentAttributes.of({
            "aria-label": label,
            "aria-multiline": "true",
          }),
          EditorView.theme({
            "&": { height: "100%", fontSize: "13px" },
            ".cm-scroller": {
              overflow: "auto",
              fontFamily: "Consolas, monospace",
            },
            ".cm-content": { minHeight: "240px" },
            ".cm-focused": { outline: "none" },
          }),
          EditorView.updateListener.of((update) => {
            if (
              update.docChanged &&
              update.transactions.some(
                (transaction) =>
                  transaction.isUserEvent("input") ||
                  transaction.isUserEvent("delete") ||
                  transaction.isUserEvent("undo") ||
                  transaction.isUserEvent("redo") ||
                  transaction.isUserEvent("move"),
              )
            )
              change.current?.(update.state.doc.toString());
          }),
        ],
      }),
    });
    view.current = editor;
    return () => {
      view.current = null;
      editor.destroy();
    };
  }, [language, readOnly, label]);
  useEffect(() => {
    const editor = view.current;
    if (editor && editor.state.doc.toString() !== value)
      editor.dispatch({
        changes: { from: 0, to: editor.state.doc.length, insert: value },
        annotations: Transaction.addToHistory.of(false),
      });
  }, [value]);
  useEffect(() => {
    const editor = view.current;
    if (editor && line) {
      const location = editor.state.doc.line(
        Math.max(1, Math.min(line, editor.state.doc.lines)),
      ).from;
      editor.dispatch({
        selection: { anchor: location },
        effects: EditorView.scrollIntoView(location, { y: "center" }),
      });
      editor.focus();
    }
  }, [line]);
  return <div className="code-editor" ref={host} />;
}
