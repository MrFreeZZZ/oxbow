# Настройки Oxbow: структура и группировка

Дата: 30 октября 2026. Анализ и согласованная структура для реализации.

После просмотра трёх макетов пользователь поручил реализовать перекомпоновку.
Отдельное уточнение пользователя: **Restore defaults** и **Edit in settings.json**
сохранить в шапке на всех страницах, как сейчас. Меню «…» из макетов не переносить.

Реализовано локально 30 октября: структура ниже перенесена в приложение; существующие
ключи, значения по умолчанию и обработчики сохранены. Проверки интерфейса и нативная
macOS-сборка прошли. Задержка Opening… разрешилась после того, как пользователь
подтвердил запрос доступа к файлам. Окончательный пакет открыл репозиторий;
настройки текущего репозитория и новые группы проверены повторно. Подробности:
`../design-qa.md`. Публикация сборки и push не выполнялись.

Основание: просмотр всех десяти пунктов Settings в запущенном macOS-приложении,
снимки экранов в текущей сессии и проверка соответствующей структуры в локальном коде.
Версия установленного приложения не сопоставлялась с Git SHA; состав просмотренных
разделов совпадает с локальными определениями. Изменения настроек не выполнялись.

## Вывод

Нужна умеренная перекомпоновка. Боковая панель, группы строк, поиск, предпросмотры
и отделение This Repository уже дают хорошую основу. Основная проблема — распределение
по темам: General перегружен, а настройки внешнего вида, сети и безопасности разнесены.
Рекомендации ниже основаны на экспертном просмотре; пользовательское тестирование
поиска настроек не проводилось.

## Предлагаемая структура

| Раздел | Группы в порядке отображения |
| --- | --- |
| General | Startup; Learning |
| Appearance (вместо Themes) | Interface: System/Light/Dark и History rows; Code theme: Variant, выбор темы и пример; Branches & graph: палитра и пример |
| Accounts | GitHub; GitLab; SSH keys |
| Git | Commit identity; Signing; Fetch; Pull; Commits; Git executable & output |
| ↳ Branches | Branch behavior; New branch names; Where a new branch starts; Prefixes |
| Diff & Text | Diff display; File folding & large diffs; Blame; Code text; Console & Activity |
| Safety & Recovery (новый) | Confirmations; Secrets check; Repository Trust; Undo history; Local History |
| Keyboard Shortcuts | Существующие категории команд |
| External Tools (вместо Integrations) | Editor; Terminal |
| This Repository → имя репозитория | Identity; Remotes; Branches: Production branch и Protected Branches; Checks Before Commit; Storage |
| ↳ Repository Files | Существующий список общих файлов; пояснение о записи в рабочую копию перед списком |

Это на один самостоятельный раздел больше текущего меню. Короткие General и External Tools
допустимы: их не нужно искусственно заполнять. Шрифты остаются в Diff & Text рядом
с настройками чтения кода; Appearance отвечает за оформление окна и цветовые темы.

## Приоритетные изменения

1. **Собрать внешний вид.** System/Light/Dark и History rows перенести из General
   в Appearance. Сейчас Variant и палитра ссылаются на Appearance в другом разделе.
   Выбор режима должен предшествовать зависящим от него темам.
2. **Собрать безопасность и восстановление.** Confirmations и Safety из General,
   Security и Repository Trust из Git перенести в Safety & Recovery. Ask before force
   pushing поставить в Confirmations, рядом с общей политикой подтверждений.
   Защищённые ветки и команды проверок оставить локальными в This Repository.
3. **Собрать сетевое поведение.** В Git → Fetch поместить auto-fetch, интервал
   и fetch.prune из Branches. В Pull сохранить fetch перед pull, auto-stash,
   режим pull и обновление подмодулей: это части конкретного действия Pull.
   Fetch first при создании ветки оставить в Branches — у него отдельный сценарий.
4. **Разделить вход и авторство.** Commit identity и Signing перенести из Accounts
   в Git, SSH keys оставить в Accounts как средство доступа к серверам.
   Добавить переход между Signing и SSH keys, чтобы выбор ключа оставался понятен.
5. **Перегруппировать Diff.** Отделить Show/Layout/Context/Highlight/Wrap/Whitespace
   от Files of a commit/Fold over/Hide over. Переименовать Text в Code text;
   примеры размещать внутри соответствующей группы, чтобы два заголовка Preview
   не разрывали связь между образцом и его параметрами.
6. **Исправить группировку локальных веток.** Production branch сейчас находится
   в Graph, хотя настройки создания веток тоже используют её как исходную.
   Объединить её с Protected Branches в Branches и описать оба назначения.
7. **Уточнить названия.** Integrations содержит только редактор и терминал,
   поэтому External Tools точнее и не конкурирует с Accounts. Language в General
   содержит только язык вывода Git: перенести в Git executable & output,
   не создавать впечатление, что это выбор языка интерфейса.

## Дополнительные замечания

- Restore defaults и Edit in settings.json видны даже в настройках репозитория,
  хотя действуют на настройки Oxbow, а не на весь показанный локальный Git config.
  **По уточнению пользователя оставить названия и видимость без изменений.**
  Первоначальное предложение переместить их в общее меню отменено.
  Кнопки во время анализа не нажимались; область действия проверена в SettingsWindow.svelte.
- В Repository Files пояснение «Save only writes the file» находится после списка.
  Поднять его выше: это важное отличие от личных настроек с немедленным сохранением.
- Общие настройки явно обозначать All repositories, локальные — This repository.
  Для Git config сообщать, что изменение действует и в Terminal. Перегруппировка
  не должна менять ключи, значения, уровень хранения или существующие разрешения.
- Длинные пояснения мелким серым текстом создают риск плохой читаемости; численный
  контраст не измерялся. В частности, пояснение о совместных файлах нужно сделать
  более заметным, а не просто передвинуть.
- В полученном дереве доступности отсутствовали два видимых пункта навигации
  This Repository; они открывались кликом по координатам. Это наблюдение требует
  отдельной проверки VoiceOver и клавиатурой, а не доказывает дефект доступности.

## Пройденные экраны и доказательства

Снимки сохранены локально вне репозитория в каталоге текущей сессии
`/Users/aleksandrmarin/.codex/visualizations/2026/10/30/01a18bf1-52eb-7741-ad8c-25f7fe2d41d2/settings-audit/`.
Они содержат пользовательские данные интерфейса; не включать их в публичный Git.
Каждый приведённый снимок был сохранён и открыт для проверки.

1. **General — требует перегруппировки.** Семь разнородных групп; Safety ниже первого экрана.

   ![General](/Users/aleksandrmarin/.codex/visualizations/2026/10/30/01a18bf1-52eb-7741-ad8c-25f7fe2d41d2/settings-audit/01-general.png)

   ![Confirmations и Safety](/Users/aleksandrmarin/.codex/visualizations/2026/10/30/01a18bf1-52eb-7741-ad8c-25f7fe2d41d2/settings-audit/11-general-safety.png)

2. **Themes — хорошая основа, зависимость от General.** Есть примеры и варианты, но режим окна находится отдельно.

   ![Themes](/Users/aleksandrmarin/.codex/visualizations/2026/10/30/01a18bf1-52eb-7741-ad8c-25f7fe2d41d2/settings-audit/02-themes.png)

3. **Accounts — смешаны вход и авторство.** GitHub/GitLab, Commit identity, SSH, Signing.

   ![Accounts](/Users/aleksandrmarin/.codex/visualizations/2026/10/30/01a18bf1-52eb-7741-ad8c-25f7fe2d41d2/settings-audit/03-accounts.png)

4. **Git — требует перегруппировки.** Путь Git стоит раньше повседневных параметров; безопасность смешана с поведением команд.

   ![Git](/Users/aleksandrmarin/.codex/visualizations/2026/10/30/01a18bf1-52eb-7741-ad8c-25f7fe2d41d2/settings-audit/04-git.png)

5. **Branches — в основном удачно.** Сохранить подраздел Git и последовательность behavior → names → start → prefixes; prune перенести к fetch.

   ![Branches](/Users/aleksandrmarin/.codex/visualizations/2026/10/30/01a18bf1-52eb-7741-ad8c-25f7fe2d41d2/settings-audit/05-branches.png)

6. **Diff & Text — нужны более точные группы.** Одна большая группа Diff; шрифты и примеры находятся ниже Blame.

   ![Diff](/Users/aleksandrmarin/.codex/visualizations/2026/10/30/01a18bf1-52eb-7741-ad8c-25f7fe2d41d2/settings-audit/06-diff.png)

   ![Text и Console](/Users/aleksandrmarin/.codex/visualizations/2026/10/30/01a18bf1-52eb-7741-ad8c-25f7fe2d41d2/settings-audit/12-text-console.png)

7. **Keyboard Shortcuts — структуру сохранить.** Категории команд, отдельный поиск и фильтр Changed уместны. Переназначения не проверялись.

   ![Keyboard Shortcuts](/Users/aleksandrmarin/.codex/visualizations/2026/10/30/01a18bf1-52eb-7741-ad8c-25f7fe2d41d2/settings-audit/07-shortcuts.png)

8. **Integrations — достаточно переименования.** Два понятных пункта Editor и Terminal.

   ![Integrations](/Users/aleksandrmarin/.codex/visualizations/2026/10/30/01a18bf1-52eb-7741-ad8c-25f7fe2d41d2/settings-audit/08-integrations.png)

9. **This Repository — область выделена удачно, группы нужно переставить.** Production branch и защита веток разнесены; общие действия в шапке неоднозначны.

   ![This Repository](/Users/aleksandrmarin/.codex/visualizations/2026/10/30/01a18bf1-52eb-7741-ad8c-25f7fe2d41d2/settings-audit/09-repository.png)

10. **Repository Files — структуру сохранить, пояснение поднять.** Файлы явно перечислены; редактирование и сохранение не выполнялись.

    ![Repository Files](/Users/aleksandrmarin/.codex/visualizations/2026/10/30/01a18bf1-52eb-7741-ad8c-25f7fe2d41d2/settings-audit/10-repository-files.png)

## Источники и ограничения

- `app/src/lib/SettingsWindow.svelte`: порядок разделов, область глобальных действий,
  вставка предпросмотров, поиск.
- `app/src/lib/settingsSections.ts`: General, Themes, Diff & Text, Integrations.
- `app/src/lib/settingsGit.ts`: Accounts, Git, This Repository.
- `app/src/lib/settingsBranches.ts`: правила создания веток и префиксы.
- `app/src/lib/settingsKeys.svelte.ts`: категории сочетаний клавиш.
- `app/src/lib/repoFileState.svelte.ts`: группы общих файлов репозитория.

Просмотр проводился в светлом оформлении при текущем размере окна. Полный проход
VoiceOver, клавиатурной навигации, масштаба, тёмной темы и сценариев сохранения
не выполнялся. Это анализ структуры настроек, а не проверка всех их функций.
На этапе исходного аудита сборка и тесты не запускались, поскольку код приложения
ещё не менялся. Проверки последующей реализации описаны в `../design-qa.md`.
