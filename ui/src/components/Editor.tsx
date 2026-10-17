import { useEffect, useRef, useImperativeHandle, forwardRef } from "react";
import { EditorView, Decoration, type DecorationSet } from "@codemirror/view";
import {
  EditorState,
  StateEffect,
  StateField,
  Annotation,
  Transaction,
} from "@codemirror/state";
import { basicSetup } from "codemirror";
import {
  StreamLanguage,
  HighlightStyle,
  defaultHighlightStyle,
  syntaxHighlighting,
} from "@codemirror/language";
import { jinja2 } from "@codemirror/legacy-modes/mode/jinja2";
import { json } from "@codemirror/lang-json";
// Reuse CodeMirror's token mapping with semantic classes instead of its fixed
// light palette. CSS variables update in place when documentElement's theme
// changes, so switching themes never replaces the document or selection.
const syntaxClasses = [
  "syntax-meta",
  "syntax-link",
  "syntax-heading",
  "syntax-emphasis",
  "syntax-strong",
  "syntax-strikethrough",
  "syntax-keyword",
  "syntax-atom",
  "syntax-number",
  "syntax-string",
  "syntax-escape",
  "syntax-variable",
  "syntax-variable",
  "syntax-type",
  "syntax-type",
  "syntax-variable",
  "syntax-variable",
  "syntax-comment",
  "syntax-invalid",
];
const editorHighlightStyle = HighlightStyle.define(
  defaultHighlightStyle.specs.map(({ tag }, index) => ({
    tag,
    class: syntaxClasses[index] ?? "syntax-meta",
  })),
);
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
export interface EditorProps {
  value: string;
  onChange?: (text: string) => void;
  language?: "jinja" | "json" | "text";
  readonly?: boolean;
  label?: string;
  line?: number;
  editorKey?: string;
}
export const Editor = forwardRef<EditorHandle, EditorProps>(function Editor(
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
          syntaxHighlighting(editorHighlightStyle),
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
            "&": {
              height: "100%",
              fontSize: "13px",
              color: "var(--editor-text)",
              backgroundColor: "var(--editor-background)",
            },
            ".cm-scroller": {
              overflow: "auto",
              lineHeight: "1.7",
              fontFamily:
                '"Cascadia Code", "SFMono-Regular", Consolas, monospace',
            },
            ".cm-content": {
              padding: "12px 0",
              caretColor: "var(--editor-text)",
            },
            ".cm-line": { padding: "0 14px" },
            ".cm-gutters": {
              backgroundColor: "var(--editor-gutter)",
              color: "var(--editor-gutter-text)",
              borderRight: "1px solid var(--line)",
            },
            "&.cm-focused": { outline: "none" },
            ".cm-activeLine": {
              backgroundColor: "var(--editor-active-line)",
            },
            ".cm-activeLineGutter": {
              backgroundColor: "var(--editor-active-gutter)",
              color: "var(--editor-text)",
            },
            ".cm-cursor, .cm-dropCursor": {
              borderLeftColor: "var(--editor-text)",
            },
            "&.cm-focused > .cm-scroller > .cm-selectionLayer .cm-selectionBackground, .cm-selectionBackground":
              { backgroundColor: "var(--editor-selection)" },
            ".cm-content ::selection": {
              backgroundColor: "var(--editor-selection)",
            },
            "&.cm-focused .cm-matchingBracket, .cm-matchingBracket": {
              backgroundColor: "var(--editor-match)",
              color: "var(--editor-text)",
            },
            ".cm-searchMatch": {
              backgroundColor: "var(--editor-search)",
              outline: "1px solid var(--warning-line)",
            },
            ".cm-searchMatch.cm-searchMatch-selected": {
              backgroundColor: "var(--editor-match)",
            },
            ".cm-panels, .cm-tooltip": {
              backgroundColor: "var(--surface-muted)",
              color: "var(--text)",
              borderColor: "var(--line)",
            },
            ".cm-panels-top": { borderBottom: "1px solid var(--line)" },
            ".cm-panels-bottom": { borderTop: "1px solid var(--line)" },
            ".cm-tooltip-autocomplete > ul > li[aria-selected]": {
              backgroundColor: "var(--brand-light)",
              color: "var(--text)",
            },
            ".cm-button, .cm-textfield": {
              background: "var(--surface)",
              color: "var(--text)",
              border: "1px solid var(--line)",
            },
            ".cm-foldPlaceholder": {
              backgroundColor: "var(--surface-muted)",
              color: "var(--muted)",
              borderColor: "var(--line)",
            },
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
        // Controlled values are synchronization, not user edits. In particular,
        // undo must never restore a previous document through the new callback.
        annotations: [externalValue.of(true), Transaction.addToHistory.of(false)],
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
