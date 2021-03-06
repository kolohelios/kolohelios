// /config/config.js

/* eslint no-reserved-keys: 0 */

'use strict';

angular.module('kolohelios')
.config(['$stateProvider', '$urlRouterProvider', function($stateProvider, $urlRouterProvider){
  $urlRouterProvider.otherwise('/');

  $stateProvider
  .state('home', {url: '/', templateUrl: '/views/home/home.html', controller: 'HomeCtrl'})
  .state('contact', {url: '/contact', templateUrl: '/views/home/contact.html'})
  .state('admin', {url: '/admin', templateUrl: '/views/admin/admin.html', controller: 'AdminCtrl'})

  .state('posts', {url: '/posts', templateUrl: '/views/posts/posts.html', abstract: true})
  .state('posts.list', {url: '', templateUrl: '/views/posts/posts_list.html', controller: 'PostsListCtrl'})
  .state('posts.new', {url: '/new', templateUrl: '/views/posts/posts_record.html', controller: 'PostsRecordCtrl'})
  .state('posts.show', {url: '/{post}', templateUrl: '/views/posts/posts_show.html', controller: 'PostsShowCtrl'})
  .state('posts.edit', {url: '/{post}/edit', templateUrl: '/views/posts/posts_record.html', controller: 'PostsRecordCtrl'})

  .state('projects', {url: '/projects', templateUrl: '/views/projects/projects.html', abstract: true})
  .state('projects.list', {url: '', templateUrl: '/views/projects/projects_list.html', controller: 'ProjectsListCtrl'})
  .state('projects.new', {url: '/new', templateUrl: '/views/projects/projects_record.html', controller: 'ProjectsRecordCtrl'})
  .state('projects.show', {url: '/{project}', templateUrl: '/views/projects/projects_show.html', controller: 'ProjectsShowCtrl'})
  .state('projects.edit', {url: '/{project}/edit', templateUrl: '/views/projects/projects_record.html', controller: 'ProjectsRecordCtrl'});
}])
.config(['markdownConfig', function(markdownConfig){
  markdownConfig.showdown.extensions = ['github'];
}])
.config(['$locationProvider', function($locationProvider){
  $locationProvider.html5Mode(false);
  $locationProvider.hashPrefix('!');
}]);
