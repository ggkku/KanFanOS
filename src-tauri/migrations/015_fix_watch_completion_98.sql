-- 完成阈值改为「至少 98% 且剩余不超过 15 秒」后，
-- 把旧规则/旧阈值下已经被标记完成、但按新规则尚未到结尾的记录改回未完成。
UPDATE watch_history
   SET completed = 0
 WHERE completed = 1
   AND last_position_ms > 0
   AND EXISTS (
       SELECT 1
         FROM videos v
        WHERE v.id = watch_history.video_id
          AND v.duration_ms > 0
          AND (
              watch_history.last_position_ms < v.duration_ms * 98 / 100
              OR v.duration_ms - watch_history.last_position_ms > 15000
          )
   );
