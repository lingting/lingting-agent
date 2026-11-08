import { Sender } from "@ant-design/x";

export type ComposerProps = {
  value: string;
  streaming: boolean;
  onChange: (value: string) => void;
  onSubmit: (value: string) => void;
  onCancel: () => void;
};

/** 输入区：Enter 发送、Shift + Enter 换行；生成中把发送按钮换成停止按钮。 */
export function Composer({ value, streaming, onChange, onSubmit, onCancel }: ComposerProps) {
  return (
    <div className="px-4 pt-2 pb-4">
      <Sender
        autoSize={{ maxRows: 6, minRows: 1 }}
        loading={streaming}
        placeholder="输入消息，Enter 发送，Shift + Enter 换行"
        submitType="enter"
        value={value}
        onCancel={onCancel}
        onChange={onChange}
        onSubmit={onSubmit}
      />
    </div>
  );
}
