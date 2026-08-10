UPDATE user_mails
SET mail_id = 0,
    sender = json_object(
        'zh', sender, 'tw', sender, 'en', sender, 'kr', sender,
        'jp', sender, 'de', sender, 'fr', sender, 'thai', sender
    ),
    title = json_object(
        'zh', title, 'tw', title, 'en', title, 'kr', title,
        'jp', title, 'de', title, 'fr', title, 'thai', title
    ),
    content = json_object(
        'zh', content, 'tw', content, 'en', content, 'kr', content,
        'jp', content, 'de', content, 'fr', content, 'thai', content
    )
WHERE mail_id != 0
  AND (
      EXISTS (
          SELECT 1
          FROM user_mail_campaign_deliveries delivery
          WHERE delivery.mail_incr_id = user_mails.incr_id
      )
      OR (mail_id = 920001 AND params = 'platform-admin')
  );
