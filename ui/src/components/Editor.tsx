import { useEffect, useRef, useImperativeHandle, forwardRef } from "react";
import { EditorView, Decoration, type DecorationSet } from "@codemirror/view";
import {
  EditorState,
  StateEffect,
  StateField,
  Annotation,
} from "@codemirror/state";
import { basicSetup } from "codemirror";
import { StreamLanguage } from "@codemirror/language";
import { jinja2 } from "@codemirror/legacy-modes/mode/jinja2";
import { json } from "@codemirror/lang-json";
const externalValue = Annotation.define<boolean>();
const highlight = StateEffect.define<number | null>();
const highlightField = StateField.define<DecorationSet>({
  create: () => Decoration.none,
  update(value, transaction) {
    value = value.map(transaction.changes);
    for (const effect of transaction.effects)
      if (effect.is(highlight)) {
        value =
          effect.value === null
            ? Decoration.none
            : Decoration.set([
                Decoration.line({ class: "editor-error-line" }).range(
                  transaction.state.doc.line(
                    Math.min(effect.value, transaction.state.doc.lines),
                  ).from,
                ),
              ]);
      }
    return value;
  },
  provide: (field) => EditorView.decorations.from(field),
});
export interface EditorHandle {
  insert: (text: string) => void;
  focus: () => void;
}
export const Editor = forwardRef<
  EditorHandle,
  {
    value: string;
    onChange?: (text: string) => void;
    language?: "jinja" | "json" | "text";
    readonly?: boolean;
    label?: string;
    line?: number;
    editorKey?: string;
  }
>(function Editor(
  {
    value,
    onChange,
    language = "jinja",
    readonly = false,
    label = "模板源码",
    line,
    editorKey = "",
  },
  ref,
) {
  const root = useRef<HTMLDivElement>(null);
  const view = useRef<EditorView | null>(null);
  const change = useRef(onChange);
  change.current = onChange;
  useImperativeHandle(
    ref,
    () => ({
      insert(text) {
        const editor = view.current;
        if (editor) {
          editor.dispatch(editor.state.replaceSelection(text));
          editor.focus();
        }
      },
      focus() {
        view.current?.focus();
      },
    }),
    [],
  );
  useEffect(() => {
    if (!root.current) return;
    const editor = new EditorView({
      parent: root.current,
      state: EditorState.create({
        doc: value,
        extensions: [
          basicSetup,
          highlightField,
          ...(language === "json"
            ? [json()]
            : language === "jinja"
              ? [StreamLanguage.define(jinja2)]
              : []),
          EditorView.lineWrapping,
          EditorState.readOnly.of(readonly),
          EditorView.editable.of(!readonly),
          EditorView.contentAttributes.of({
            "aria-label": label,
            spellcheck: "false",
          }),
          EditorView.updateListener.of((update) => {
            if (
              update.docChanged &&
              !update.transactions.some((transaction) =>
                transaction.annotation(externalValue),
              )
            )
              change.current?.(update.state.doc.toString());
          }),
          EditorView.theme({
            "&": { height: "100%", fontSize: "13px" },
            ".cm-scroller": {
              overflow: "auto",
              fontFamily:
                '"Cascadia Code", "SFMono-Regular", Consolas, monospace',
            },
            ".cm-content": { padding: "14px 0" },
            ".cm-line": { padding: "0 16px" },
            ".cm-gutters": {
              backgroundColor: "#fafbfc",
              color: "#99a3af",
              borderRight: "1px solid #eef0f3",
            },
            "&.cm-focused": { outline: "none" },
            ".cm-activeLine": { backgroundColor: "#f4f7fa" },
            ".cm-activeLineGutter": { backgroundColor: "#eef3f8" },
          }),
        ],
      }),
    });
    view.current = editor;
    return () => {
      editor.destroy();
      view.current = null;
    };
  }, [language, readonly, label, editorKey]);
  useEffect(() => {
    const editor = view.current;
    if (editor && editor.state.doc.toString() !== value)
      editor.dispatch({
        changes: { from: 0, to: editor.state.doc.length, insert: value },
        annotations: externalValue.of(true),
      });
  }, [value]);
  useEffect(() => {
    const editor = view.current;
    if (editor) {
      editor.dispatch({ effects: highlight.of(line || null) });
      if (line && line > 0) {
        const from = editor.state.doc.line(
          Math.min(line, editor.state.doc.lines),
        ).from;
        editor.dispatch({
          selection: { anchor: from },
          effects: EditorView.scrollIntoView(from, { y: "center" }),
        });
      }
    }
  }, [line]);
  return <div ref={root} className="code-editor" />;
});
