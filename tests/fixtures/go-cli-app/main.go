package main

import (
	"errors"
	"fmt"
	"strings"
	"time"
)

// Task represents a single to-do item.
type Task struct {
	ID          int
	Title       string
	Description string
	Done        bool
	CreatedAt   time.Time
	CompletedAt *time.Time
	Tags        []string
	Priority    int
}

// TaskStore defines the interface for task persistence.
type TaskStore interface {
	Save(task *Task) error
	FindByID(id int) (*Task, error)
	FindAll() ([]*Task, error)
	Delete(id int) error
}

// TaskService manages task operations.
type TaskService struct {
	store   TaskStore
	nextID  int
}

// NewTaskService creates a new service with the given store.
func NewTaskService(store TaskStore) *TaskService {
	return &TaskService{
		store:  store,
		nextID: 1,
	}
}

// CreateTask creates a new task with validation.
func (s *TaskService) CreateTask(title, description string, tags []string) (*Task, error) {
	title = strings.TrimSpace(title)
	if title == "" {
		return nil, errors.New("task title cannot be empty")
	}
	if len(title) > 200 {
		return nil, errors.New("task title too long (max 200 chars)")
	}

	task := &Task{
		ID:          s.nextID,
		Title:       title,
		Description: strings.TrimSpace(description),
		Done:        false,
		CreatedAt:   time.Now(),
		Tags:        normalizeTags(tags),
		Priority:    0,
	}
	s.nextID++

	if err := s.store.Save(task); err != nil {
		return nil, fmt.Errorf("failed to save task: %w", err)
	}

	return task, nil
}

// CompleteTask marks a task as done.
func (s *TaskService) CompleteTask(id int) error {
	task, err := s.store.FindByID(id)
	if err != nil {
		return fmt.Errorf("task not found: %w", err)
	}

	if task.Done {
		return errors.New("task already completed")
	}

	now := time.Now()
	task.Done = true
	task.CompletedAt = &now

	return s.store.Save(task)
}

// ListPending returns all incomplete tasks sorted by priority.
func (s *TaskService) ListPending() ([]*Task, error) {
	all, err := s.store.FindAll()
	if err != nil {
		return nil, err
	}

	var pending []*Task
	for _, t := range all {
		if !t.Done {
			pending = append(pending, t)
		}
	}

	// Sort by priority (higher first)
	for i := 0; i < len(pending); i++ {
		for j := i + 1; j < len(pending); j++ {
			if pending[j].Priority > pending[i].Priority {
				pending[i], pending[j] = pending[j], pending[i]
			}
		}
	}

	return pending, nil
}

// SearchByTag finds tasks with a specific tag.
func (s *TaskService) SearchByTag(tag string) ([]*Task, error) {
	tag = strings.ToLower(strings.TrimSpace(tag))
	if tag == "" {
		return nil, errors.New("search tag cannot be empty")
	}

	all, err := s.store.FindAll()
	if err != nil {
		return nil, err
	}

	var matched []*Task
	for _, t := range all {
		for _, taskTag := range t.Tags {
			if taskTag == tag {
				matched = append(matched, t)
				break
			}
		}
	}

	return matched, nil
}

// SetPriority updates the priority of a task.
func (s *TaskService) SetPriority(id, priority int) error {
	if priority < 0 || priority > 10 {
		return errors.New("priority must be between 0 and 10")
	}

	task, err := s.store.FindByID(id)
	if err != nil {
		return err
	}

	task.Priority = priority
	return s.store.Save(task)
}

// ComputeStats returns summary statistics for all tasks.
func ComputeStats(tasks []*Task) map[string]interface{} {
	stats := map[string]interface{}{
		"total":     len(tasks),
		"done":      0,
		"pending":   0,
		"avg_title": 0.0,
	}

	if len(tasks) == 0 {
		return stats
	}

	done := 0
	totalTitleLen := 0
	for _, t := range tasks {
		if t.Done {
			done++
		}
		totalTitleLen += len(t.Title)
	}

	stats["done"] = done
	stats["pending"] = len(tasks) - done
	stats["avg_title"] = float64(totalTitleLen) / float64(len(tasks))

	return stats
}

// normalizeTags cleans and deduplicates tag strings.
func normalizeTags(tags []string) []string {
	seen := make(map[string]bool)
	var result []string
	for _, tag := range tags {
		tag = strings.ToLower(strings.TrimSpace(tag))
		if tag != "" && !seen[tag] {
			seen[tag] = true
			result = append(result, tag)
		}
	}
	return result
}
