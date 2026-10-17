<!-- 新增与编辑共用定稿弹窗；草稿和命令状态由外部会话持有。 -->
<script lang="ts">
  import type { Dialog } from "../state";
  import { dialogError } from "../state";
  import { dialogFocus } from "../dialog-focus";
  import Button from "./Button.svelte";
  import IconButton from "./IconButton.svelte";
  import "../dialog.css";

  /** oncancel 的来源决定遮罩是否允许关闭；其余回调只转发用户意图。 */
  type Props = {
    dialog: Dialog;
    oninput: (text: string) => void;
    oncancel: (source?: "explicit" | "backdrop") => void;
    onsave: () => void;
    ondelete: () => void;
    onclosed: () => void;
  };
  let { dialog, oninput, oncancel, onsave, ondelete, onclosed }: Props = $props();
  const error = $derived(dialogError(dialog));
</script>

<div class="scrim">
  <button type="button" class="dialog-backdrop" tabindex="-1" aria-label="取消弹窗" onclick={() => oncancel("backdrop")}></button>
  <dialog open class="dlg" aria-modal="true" aria-labelledby="prompt-dialog-title" tabindex="-1"
    use:dialogFocus={{ oncancel: () => oncancel(), onsave, onclosed }}>
    <div class="dlg-h">
      <h2 class="dlg-t" id="prompt-dialog-title">{dialog.kind === "edit" ? "编辑提示词" : "新增提示词"}</h2>
      <IconButton icon="close" label="关闭" onclick={() => oncancel()} />
    </div>
    <label class="field">
      <span class="lbl">内容</span>
      <textarea class="ta" rows="3" placeholder="例如：按你推荐的来" value={dialog.text} readonly={dialog.busy}
        aria-invalid={!!error} aria-describedby="prompt-dialog-hint"
        oninput={(event) => oninput(event.currentTarget.value)}></textarea>
    </label>
    <div class="field-n">
      <div id="prompt-dialog-hint" class="hint" class:err={!!error} aria-live="polite">{error ?? "Ctrl+Enter 保存"}</div>
      {#if dialog.prompt}<div class="meta">已使用 {dialog.prompt.count} 次</div>{/if}
    </div>
    <div class="dlg-f">
      <div class="dlg-fl">
        {#if dialog.kind === "edit"}<Button variant="danger" large disabled={dialog.busy} onclick={ondelete}>删除</Button>{/if}
      </div>
      <Button variant="secondary" large onclick={() => oncancel()}>取消</Button>
      <Button large disabled={!dialog.text.trim() || dialog.busy} onclick={onsave}>{dialog.busy ? "处理中…" : "保存"}</Button>
    </div>
  </dialog>
</div>
