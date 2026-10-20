import { FolderOpen } from "lucide-react";

interface SetupScreenProps {
  onChooseRoot: () => void;
}

export function SetupScreen({ onChooseRoot }: SetupScreenProps) {
  return (
    <div className="flex flex-1 flex-col items-center justify-center py-24 text-center">
      <div className="mb-5 flex h-16 w-16 items-center justify-center rounded-2xl bg-indigo-500/10 text-indigo-400">
        <FolderOpen size={30} />
      </div>
      <h1 className="text-xl font-semibold">先选择你的番剧目录</h1>
      <p className="mt-2 max-w-md text-sm leading-relaxed text-neutral-400">
        KanFanOS 会读取该目录下的档期文件夹（如「2026年7月新番」）和番剧文件夹，
        建立索引并生成配置文件。不会移动或修改任何原始文件。
      </p>
      <button
        type="button"
        onClick={onChooseRoot}
        className="mt-6 rounded-lg bg-indigo-600 px-4 py-2 text-sm font-medium text-white transition-colors hover:bg-indigo-500"
      >
        选择番剧目录
      </button>
    </div>
  );
}
