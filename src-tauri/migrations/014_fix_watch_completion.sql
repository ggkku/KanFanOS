-- 旧逻辑在播放到 90% 时就标记看完；这里修正历史数据。
-- 只处理时长已知、已有播放位置、且位置明显未到结尾的记录。
UPDATE watch_history
   SET completed = 0
 WHERE completed = 1
   AND last_position_ms > 0
   AND EXISTS (
       SELECT 1
         FROM videos v
        WHERE v.id = watch_history.video_id
          AND v.duration_ms > 0
          AND watch_history.last_position_ms < v.duration_ms * 97 / 100
   );
