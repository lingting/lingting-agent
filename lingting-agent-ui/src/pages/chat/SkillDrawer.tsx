import XMarkdown from "@ant-design/x-markdown";
import type { SkillVO } from "@lingting/agent-api";
import { Alert, Button, Drawer, Empty, Flex, List, Spin, Switch, Tag, Typography } from "antd";
import { useState } from "react";

import { errorMessageOf } from "@/api";
import { useSkillDetail, useSkillList, useSkillUpdate } from "@/api/skill";

/** 抽屉默认宽度（D-P5-019：右侧抽屉，不打断对话）。 */
const DRAWER_WIDTH = 480;

/** 一次渲染的 Markdown 块数：大正文分块追加，避免一次性铺满 DOM。 */
const CHUNKS_PER_PAGE = 8;

/** 抽屉的开关由聊天页持有：关闭即收起，不影响输入框里的草稿。 */
export type SkillDrawerProps = { onClose: () => void; open: boolean };

/** 状态标签：`INVALID` 先说原因，禁用是「可用但没启用」，与 `READY` 区分。 */
function statusTag(skill: SkillVO) {
  if (skill.status === "INVALID") return <Tag color="error">INVALID</Tag>;
  if (!skill.enabled) return <Tag>禁用</Tag>;
  return <Tag color="success">READY</Tag>;
}

/**
 * 技能抽屉：清单 + 详情 + 启停 + 重载。
 *
 * 只做四件事（D-P5-019）：列出、查看正文、启停、重载；编辑与新建都不在这里，文件系统仍是唯一事实源。
 * 动作成功后服务端返回整张清单，直接写回缓存，不再二次请求。
 */
export function SkillDrawer({ onClose, open }: SkillDrawerProps) {
  const list = useSkillList();
  const update = useSkillUpdate();
  const [selected, setSelected] = useState<string | null>(null);

  const skills = list.data ?? [];
  const current = skills.find((skill) => skill.name === selected) ?? null;
  const detail = useSkillDetail(current ? current.name : null);
  const failure = list.error ?? update.error;

  const reload = (
    <Button
      loading={update.isPending}
      size="small"
      onClick={() => update.mutate({ action: "RELOAD" })}
    >
      重载
    </Button>
  );

  const toggle = (skill: SkillVO, enabled: boolean) => {
    update.mutate({ action: enabled ? "ENABLE" : "DISABLE", name: skill.name });
  };

  return (
    <Drawer extra={reload} onClose={onClose} open={open} size={DRAWER_WIDTH} title="技能">
      <Flex gap={12} vertical>
        {failure && <Alert closable message={errorMessageOf(failure)} type="error" />}
        {list.isPending ? (
          <Flex justify="center" className="py-8">
            <Spin />
          </Flex>
        ) : skills.length === 0 ? (
          <Empty description="没有发现技能" />
        ) : (
          <List
            dataSource={skills}
            renderItem={(skill) => (
              <SkillRow
                busy={update.isPending}
                onSelect={setSelected}
                onToggle={toggle}
                selected={skill.name === selected}
                skill={skill}
              />
            )}
            size="small"
          />
        )}
        {current && (
          <SkillDetail
            body={detail.data?.body ?? ""}
            key={current.name}
            loading={detail.isPending}
            references={detail.data?.references ?? []}
            skill={current}
          />
        )}
      </Flex>
    </Drawer>
  );
}

/** 一行清单项：名字、状态、缺失工具与启用开关；禁用项置灰但仍可点开详情。 */
function SkillRow({
  busy,
  onSelect,
  onToggle,
  selected,
  skill,
}: {
  busy: boolean;
  onSelect: (name: string) => void;
  onToggle: (skill: SkillVO, enabled: boolean) => void;
  selected: boolean;
  skill: SkillVO;
}) {
  return (
    <List.Item
      className={`cursor-pointer rounded px-2 ${selected ? "bg-black/5" : ""}`}
      actions={
        skill.status === "READY"
          ? [
              // 开关的点击不冒泡到行上：点开关只切换启用，不切换详情。
              <span key="switch" onClick={(event) => event.stopPropagation()}>
                <Switch
                  checked={skill.enabled}
                  disabled={busy}
                  onChange={(checked) => onToggle(skill, checked)}
                  size="small"
                />
              </span>,
            ]
          : []
      }
      onClick={() => onSelect(skill.name)}
    >
      <List.Item.Meta
        description={
          <Flex gap={4} wrap>
            {statusTag(skill)}
            {skill.missingTools.length > 0 && (
              <Tag color="warning">缺 {skill.missingTools.join("、")}</Tag>
            )}
            <Typography.Text type="secondary">{skill.directory}</Typography.Text>
          </Flex>
        }
        title={
          <Typography.Text strong type={skill.enabled ? undefined : "secondary"}>
            {skill.name}
          </Typography.Text>
        }
      />
    </List.Item>
  );
}

/** 详情区：`INVALID` 给可定位原因，其余渲染正文（分块）与引用清单。 */
function SkillDetail({
  body,
  loading,
  references,
  skill,
}: {
  body: string;
  loading: boolean;
  references: string[];
  skill: SkillVO;
}) {
  const [pages, setPages] = useState(1);
  const blocks = splitBlocks(body);
  const shown = blocks.slice(0, pages * CHUNKS_PER_PAGE).join("\n\n");
  const remaining = blocks.length - pages * CHUNKS_PER_PAGE;

  if (skill.status === "INVALID") {
    return (
      <Flex gap={6} vertical>
        <Typography.Text strong>{skill.name}</Typography.Text>
        <Alert
          description={skill.error ?? "没有给出原因"}
          message="解析失败：不进索引，也不可加载"
          type="error"
        />
      </Flex>
    );
  }

  return (
    <Flex gap={6} vertical>
      <Typography.Text strong>{skill.name}</Typography.Text>
      <Typography.Text type="secondary">{skill.description}</Typography.Text>
      {loading ? (
        <Flex justify="center" className="py-6">
          <Spin />
        </Flex>
      ) : !skill.enabled ? (
        <Typography.Text type="secondary">
          已禁用：正文不进索引，启用后可在对话中加载。
        </Typography.Text>
      ) : (
        <>
          <div className="max-h-[50vh] overflow-y-auto">
            <XMarkdown content={shown} />
          </div>
          {remaining > 0 && (
            <Button block size="small" onClick={() => setPages(pages + 1)}>
              继续显示（还有 {remaining} 段）
            </Button>
          )}
        </>
      )}
      {references.length > 0 && (
        <Flex gap={4} vertical>
          <Typography.Text type="secondary">
            参考资料（点击复制路径，用 file_read 读取）
          </Typography.Text>
          {references.map((reference) => (
            <Typography.Text code copyable key={reference}>
              {reference}
            </Typography.Text>
          ))}
        </Flex>
      )}
    </Flex>
  );
}

/** 正文按空行切块：分块渲染的最小单位，标点与代码块内容都不改写。 */
function splitBlocks(body: string): string[] {
  const text = body.trim();
  if (!text) return [];
  return text.split(/\n{2,}/);
}
