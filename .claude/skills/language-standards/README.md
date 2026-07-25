# Skill Language Standards

Claude Code 的 Skill，用于指导 Agent 在与人类对话时始终使用中文。

## 安装

```bash
npx skills add https://github.com/EchoLab-Auto/skill-language-standards --skill language-standards --agent claude-code -y
```

## Skill 说明

`language-standards` skill 会指导 Claude Code Agent：

- **始终使用中文回复**，无论用户用什么语言提问
- **保持技术术语的准确性**，通用技术名词保留英文
- **代码和命令保持原样**，只翻译说明和注释
- **使用礼貌专业的沟通风格**，用"您"称呼用户
- **主动确认需求**，在开始任务前复述理解

## 适用场景

- 中文开发团队使用 Claude Code 进行日常开发
- 需要用中文进行技术交流和代码审查
- 非英语母语者希望获得更自然的中文交互体验

## 许可证

MIT
