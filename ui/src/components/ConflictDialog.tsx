import type { Asset } from "../lib/data";

interface ConflictDialogProps {
  draftSource: string;
  diskAsset?: Asset;
  onContinue: () => void;
  onLoadDisk: () => void;
  onSaveCopy: () => void;
}

export function ConflictDialog({
  draftSource,
  diskAsset,
  onContinue,
  onLoadDisk,
  onSaveCopy,
}: ConflictDialogProps) {
  return (
    <>
      <p>当前草稿保留。请比较两个版本，再决定如何处理。</p>
      <div className="conflict-columns">
        <div>
          <strong>当前草稿</strong>
          <pre>{draftSource}</pre>
        </div>
        <div>
          <strong>磁盘版本</strong>
          <pre>{diskAsset?.source}</pre>
        </div>
      </div>
      <div className="modal-footer">
        <button className="button secondary" onClick={onContinue}>
          继续编辑
        </button>
        <button className="button secondary" onClick={onLoadDisk}>
          加载磁盘版本
        </button>
        <button className="button primary" onClick={onSaveCopy}>
          另存为副本
        </button>
      </div>
    </>
  );
}
